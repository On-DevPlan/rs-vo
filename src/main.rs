// Windows 会议录音工具 - 入口
//
// 线程模型
//   主线程  : CLI 按键轮询 + 屏幕状态 + i16 整数混音写盘
//   线程 A  : 默认麦克风 (Capture 设备) -> tx_mic: SyncSender<Vec<i16>>
//   线程 B  : 默认扬声器 (Render 设备, loopback) -> tx_sys: SyncSender<Vec<i16>>
//
// 关键设计:
//   * 两路均使用 WASAPI 共享模式 + AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
//     系统自动重采样 / 降混到统一目标格式 (16-bit PCM, 双声道, 44.1/48 kHz).
//   * 每个采集线程每 50ms 排出恰好 samples_per_tick 个 i16 样本
//     (不足补静音), 两路时间和样本数天然对齐.
//   * 主线程 i32 整数加和 + clamp [-32768, 32767], 直接写 hound, 没有浮点转换.
//   * 全程不切换到独占模式, 不影响 Zoom/Teams 的播放与麦克风使用.

mod audio;
mod ui;
mod writer;

use anyhow::{Context, Result};
use chrono::Local;
use clap::Parser;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use audio::AudioFormat;
use ui::BannerInfo;

/// Ctrl+C 中断标志
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

#[derive(Parser, Debug)]
#[command(name = "rs-vo", version, about = "Windows 会议录音工具")]
struct Args {
    /// 列出所有音频设备后退出
    #[arg(long)]
    list_devices: bool,

    /// 输出目录 (默认: %USERPROFILE%\Documents\Recordings)
    #[arg(long)]
    out: Option<PathBuf>,

    /// 文件名前缀
    #[arg(long, default_value = "rec")]
    name: String,

    /// 输出采样率, 44100 或 48000
    #[arg(long, default_value_t = 44100)]
    rate: u32,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // COM 初始化必须在任何 WASAPI 调用之前 (--list-devices 也用得到)
    wasapi::initialize_mta()
        .ok()
        .context("COM 初始化失败")?;

    if args.list_devices {
        return audio::list_devices();
    }

    // Ctrl+C 优雅退出
    ctrlc::set_handler(|| {
        INTERRUPTED.store(true, Ordering::SeqCst);
    })
    .context("无法设置 Ctrl+C 处理器")?;

    // 输出目录: CLI 参数 > RS_VO_OUT 环境变量 > 默认 ~/Documents/Recordings
    let out_dir = args
        .out
        .clone()
        .or_else(|| {
            std::env::var("RS_VO_OUT")
                .ok()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| dirs::document_dir().map(|p| p.join("Recordings")))
        .context("无法确定输出目录, 请用 --out 指定或设置 RS_VO_OUT")?;
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("无法创建输出目录: {}", out_dir.display()))?;

    // 文件名前缀: CLI 参数 > RS_VO_NAME 环境变量 > 默认 "rec"
    let name = std::env::var("RS_VO_NAME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| args.name.clone());

    // 采样率校验
    let sample_rate = match args.rate {
        44100 | 48000 => args.rate,
        _ => anyhow::bail!("--rate 仅支持 44100 或 48000"),
    };

    let mic_name = audio::get_default_device_name(audio::Direction::Capture)
        .unwrap_or_else(|_| "(未知)".into());
    let loopback_name = audio::get_default_device_name(audio::Direction::Render)
        .unwrap_or_else(|_| "(未知)".into());

    let _terminal_guard = ui::TerminalGuard;
    ui::init_terminal().context("进入终端 raw 模式失败")?;
    ui::print_banner(&BannerInfo {
        version: env!("CARGO_PKG_VERSION"),
        mic: &mic_name,
        loopback: &loopback_name,
        out_dir: &out_dir,
        sample_rate,
    });

    // 共享状态
    let recording = Arc::new(AtomicBool::new(false));
    let (tx_sys, rx_sys) = mpsc::sync_channel::<Vec<i16>>(64);
    let (tx_mic, rx_mic) = mpsc::sync_channel::<Vec<i16>>(64);
    let format = AudioFormat { sample_rate, channels: 2 };

    // 每个 50ms tick 的样本数, 主/采集线程一致
    let samples_per_tick =
        sample_rate as usize * 50 / 1000 * format.channels as usize;

    // 录音上下文
    let mut wav_writer: Option<writer::WavWriterHandle> = None;
    let mut current_path: Option<PathBuf> = None;
    let mut started_at: Option<Instant> = None;
    let mut last_refresh: Option<Instant> = None;
    let mut sys_buf: Vec<i16> = Vec::new();
    let mut mic_buf: Vec<i16> = Vec::new();

    loop {
        // Ctrl+C 中断检查
        if INTERRUPTED.load(Ordering::SeqCst) {
            if recording.load(Ordering::Relaxed) {
                stop_and_save(
                    &recording,
                    &mut wav_writer,
                    &mut current_path,
                    &mut started_at,
                    &mut sys_buf,
                    &mut mic_buf,
                    &rx_sys,
                    &rx_mic,
                    samples_per_tick,
                )?;
            }
            break;
        }

        // 轮询按键
        if let Some(key) = ui::poll_key(Duration::from_millis(100))? {
            match key {
                'y' => {
                    if recording.load(Ordering::Relaxed) {
                        ui::warn("已在录音中");
                        continue;
                    }
                    let now = Local::now();
                    let filename =
                        format!("{}_{}.wav", name, now.format("%Y%m%d_%H%M%S"));
                    let path = out_dir.join(filename);

                    let w = writer::WavWriterHandle::create(&path, sample_rate)
                        .with_context(|| format!("无法创建 WAV: {}", path.display()))?;
                    wav_writer = Some(w);
                    current_path = Some(path);
                    started_at = Some(Instant::now());
                    last_refresh = Some(Instant::now());
                    sys_buf.clear();
                    mic_buf.clear();

                    recording.store(true, Ordering::Relaxed);
                    audio::spawn_capture(
                        recording.clone(),
                        tx_sys.clone(),
                        format,
                        audio::Direction::Render,
                        "loopback".to_string(),
                    );
                    audio::spawn_capture(
                        recording.clone(),
                        tx_mic.clone(),
                        format,
                        audio::Direction::Capture,
                        "mic".to_string(),
                    );
                }
                'n' => {
                    if !recording.load(Ordering::Relaxed) {
                        ui::warn("尚未开始录音");
                        continue;
                    }
                    stop_and_save(
                        &recording,
                        &mut wav_writer,
                        &mut current_path,
                        &mut started_at,
                        &mut sys_buf,
                        &mut mic_buf,
                        &rx_sys,
                        &rx_mic,
                        samples_per_tick,
                    )?;
                }
                'q' | '\u{3}' => {
                    if recording.load(Ordering::Relaxed) {
                        if !ui::confirm_discard() {
                            continue;
                        }
                        recording.store(false, Ordering::Relaxed);
                        thread::sleep(Duration::from_millis(200));
                        if let Some(w) = wav_writer.take() {
                            let _ = w.finish();
                        }
                    }
                    break;
                }
                _ => {}
            }
        }

        // 录音中: 排空通道 -> 对齐 -> 整数混音 -> 写盘
        if recording.load(Ordering::Relaxed) {
            while let Ok(p) = rx_sys.try_recv() {
                sys_buf.extend_from_slice(&p);
            }
            while let Ok(p) = rx_mic.try_recv() {
                mic_buf.extend_from_slice(&p);
            }

            // 两路同时有 samples_per_tick 才混一个 tick
            while sys_buf.len() >= samples_per_tick && mic_buf.len() >= samples_per_tick {
                if let Some(w) = wav_writer.as_mut() {
                    for i in 0..samples_per_tick {
                        let mixed = (sys_buf[i] as i32 + mic_buf[i] as i32)
                            .clamp(-32768, 32767) as i16;
                        w.write_sample(mixed)?;
                    }
                }
                sys_buf.drain(..samples_per_tick);
                mic_buf.drain(..samples_per_tick);
            }

            // 防止单路长期堆积 (例如某一路持续静默导致另一边积压)
            let max_buf = samples_per_tick * 4;
            if sys_buf.len() > max_buf {
                sys_buf.drain(..sys_buf.len() - samples_per_tick);
            }
            if mic_buf.len() > max_buf {
                mic_buf.drain(..mic_buf.len() - samples_per_tick);
            }

            // 状态行每秒刷新
            if let (Some(start), Some(last)) = (started_at, last_refresh) {
                if last.elapsed() >= Duration::from_secs(1) {
                    let elapsed = start.elapsed();
                    let size = current_path
                        .as_ref()
                        .and_then(|p| std::fs::metadata(p).ok())
                        .map(|m| m.len())
                        .unwrap_or(0);
                    ui::set_recording_status(elapsed, size);
                    last_refresh = Some(Instant::now());
                }
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn stop_and_save(
    recording: &Arc<AtomicBool>,
    wav_writer: &mut Option<writer::WavWriterHandle>,
    current_path: &mut Option<PathBuf>,
    started_at: &mut Option<Instant>,
    sys_buf: &mut Vec<i16>,
    mic_buf: &mut Vec<i16>,
    rx_sys: &mpsc::Receiver<Vec<i16>>,
    rx_mic: &mpsc::Receiver<Vec<i16>>,
    samples_per_tick: usize,
) -> Result<()> {
    recording.store(false, Ordering::Relaxed);
    thread::sleep(Duration::from_millis(300));

    ui::print_stopping();

    while let Ok(p) = rx_sys.try_recv() {
        sys_buf.extend_from_slice(&p);
    }
    while let Ok(p) = rx_mic.try_recv() {
        mic_buf.extend_from_slice(&p);
    }

    while sys_buf.len() >= samples_per_tick && mic_buf.len() >= samples_per_tick {
        if let Some(w) = wav_writer.as_mut() {
            for i in 0..samples_per_tick {
                let mixed = (sys_buf[i] as i32 + mic_buf[i] as i32)
                    .clamp(-32768, 32767) as i16;
                w.write_sample(mixed)?;
            }
        }
        sys_buf.drain(..samples_per_tick);
        mic_buf.drain(..samples_per_tick);
    }

    let path = current_path
        .take()
        .context("录音已开始但找不到路径")?;
    let elapsed = started_at.take().map(|t| t.elapsed()).unwrap_or_default();
    if let Some(w) = wav_writer.take() {
        w.finish().context("关闭 WAV 文件失败")?;
    }
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    ui::print_saved(&path, elapsed, size);

    Ok(())
}