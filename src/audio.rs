// WASAPI 采集线程 (适配 wasapi 0.20 API)
//
// 设计要点:
//   * 两路都用 StreamMode::EventsShared { autoconvert: true, .. } ——
//     shared 共享模式 + AUTOCONVERTPCM, 系统自动重采样/降混, 不抢设备.
//   * loopback: device 取 Render (扬声器), 初始化方向仍是 Capture,
//     WASAPI 检测到 (Render, Capture, Shared) 自动进入 loopback 模式.
//   * 50ms tick: 把 tick 内所有可用 packet 全部排空后, 取恰好 N 个 i16 样本
//     (不足补静音). 每路每 50ms 产出恰好 samples_per_tick 个样本,
//     时间和样本数都对齐, 主线程混音不用想时间轴.
//   * 用 i16 PCM (而不是 Float32) —— 跨设备最稳, 字节布局最简单.

use anyhow::{Context, Result};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use wasapi::{get_default_device, SampleType, WaveFormat};

// 重新导出给 main.rs 用
pub use wasapi::{Direction, StreamMode};

#[derive(Clone, Copy, Debug)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u32,
}

impl AudioFormat {
    /// 16-bit 有符号 PCM, 小端 (x86/ARM64 Windows 默认)
    pub fn to_wasapi(&self) -> WaveFormat {
        WaveFormat::new(
            16, // storebits
            16, // validbits
            &SampleType::Int,
            self.sample_rate as usize,
            self.channels as usize,
            None,
        )
    }
}

/// 取默认设备的友好名 (用于 banner 显示)
pub fn get_default_device_name(direction: Direction) -> Result<String> {
    let device = get_default_device(&direction).context("无法获取默认设备")?;
    Ok(device
        .get_friendlyname()
        .unwrap_or_else(|_| "(未知)".into()))
}

/// 枚举设备列表 (--list-devices)
pub fn list_devices() -> Result<()> {
    println!("=== 输出设备 (Render / loopback 源) ===");
    for dev in &wasapi::DeviceCollection::new(&Direction::Render).context("枚举输出设备失败")? {
        let dev = dev.context("读取输出设备失败")?;
        let name = dev.get_friendlyname().unwrap_or_else(|_| "(未知)".into());
        println!("  - {}", name);
    }
    println!();
    println!("=== 输入设备 (Capture / 麦克风) ===");
    for dev in &wasapi::DeviceCollection::new(&Direction::Capture).context("枚举输入设备失败")? {
        let dev = dev.context("读取输入设备失败")?;
        let name = dev.get_friendlyname().unwrap_or_else(|_| "(未知)".into());
        println!("  - {}", name);
    }
    Ok(())
}

/// 启动一个采集线程, 每 tick 往 tx 推一个 Vec<i16>
pub fn spawn_capture(
    running: Arc<AtomicBool>,
    tx: SyncSender<Vec<i16>>,
    format: AudioFormat,
    direction: Direction,
    name: String,
) {
    thread::Builder::new()
        .name(name.clone())
        .spawn(move || {
            if let Err(e) = run_capture(running, tx, format, direction) {
                eprintln!("[{}] 采集线程异常: {:#}", name, e);
            }
        })
        .expect("无法启动采集线程");
}

fn run_capture(
    running: Arc<AtomicBool>,
    tx: SyncSender<Vec<i16>>,
    format: AudioFormat,
    direction: Direction,
) -> Result<()> {
    let device = get_default_device(&direction).context("无法打开默认设备")?;
    let mut client = device.get_iaudioclient().context("get_iaudioclient 失败")?;

    // 2 秒缓冲, 单位 100ns. shared 模式下 buffer_duration_hns 是最小时长 hint.
    let buffer_duration_hns = 2_000_000i64;

    let stream_mode = StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns,
    };

    // 不管 device 方向是什么, 初始化方向都用 Capture.
    // WASAPI: (Render, Capture, Shared) 自动进入 loopback.
    client
        .initialize_client(
            &format.to_wasapi(),
            &Direction::Capture,
            &stream_mode,
        )
        .context("initialize_client 失败")?;

    let capture = client
        .get_audiocaptureclient()
        .context("get_audiocaptureclient 失败")?;
    let event = client
        .set_get_eventhandle()
        .context("set_get_eventhandle 失败")?;

    client.start_stream().context("start_stream 失败")?;

    const TICK_MS: u32 = 50;
    let samples_per_tick =
        format.sample_rate as usize * TICK_MS as usize / 1000 * format.channels as usize;
    let bytes_per_tick = samples_per_tick * 2; // i16 = 2 bytes

    // 内部累积队列, 跨 tick 缓存 packet 残余
    let mut queue: VecDeque<u8> = VecDeque::with_capacity(bytes_per_tick * 4);

    while running.load(Ordering::Relaxed) {
        let tick_start = Instant::now();

        // 等事件或超时. 事件触发 = 至少 1 个 packet 准备好
        let _ = event.wait_for_event(TICK_MS);

        // 把本 tick 内所有可读 packet 全部排到队列里
        loop {
            let prev = queue.len();
            if capture
                .read_from_device_to_deque(&mut queue)
                .is_err()
            {
                break;
            }
            if queue.len() == prev {
                break;
            }
        }

        // 取出恰好 bytes_per_tick 字节, 不足填 0 (静音)
        let mut chunk_bytes = vec![0u8; bytes_per_tick];
        let take_bytes = bytes_per_tick.min(queue.len());
        for i in 0..take_bytes {
            chunk_bytes[i] = queue.pop_front().unwrap();
        }

        // i16 little-endian 解码
        let chunk_i16: Vec<i16> = chunk_bytes
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();

        if tx.send(chunk_i16).is_err() {
            // 主线程已关闭
            client.stop_stream().ok();
            return Ok(());
        }

        // 维持 tick 节奏 (约 50ms 一次)
        let elapsed = tick_start.elapsed();
        if elapsed < Duration::from_millis(TICK_MS as u64) {
            thread::sleep(Duration::from_millis(TICK_MS as u64) - elapsed);
        }
    }

    client.stop_stream().ok();
    Ok(())
}