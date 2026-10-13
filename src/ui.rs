// CLI 面板: banner / 状态行 / 警告 / 按键轮询

use anyhow::Result;
use crossterm::{
    cursor::MoveTo,
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    style::Print,
    terminal::{self, Clear, ClearType},
};
use std::io::{stdout, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

/// Drop 自动 disable_raw_mode, 避免异常路径留下终端坏掉
pub struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

pub struct BannerInfo<'a> {
    pub version: &'a str,
    pub mic: &'a str,
    pub loopback: &'a str,
    pub out_dir: &'a Path,
    pub sample_rate: u32,
}

pub fn init_terminal() -> Result<()> {
    terminal::enable_raw_mode()?;
    Ok(())
}

/// banner 占 row 0..=10, 状态行在 row 11
const STATUS_ROW: u16 = 11;

pub fn print_banner(info: &BannerInfo) {
    let mut out = stdout();
    execute!(out, Clear(ClearType::All), MoveTo(0, 0)).ok();

    println!("========================================");
    println!("   Win Meeting Recorder  v{}", info.version);
    println!("========================================");
    println!("麦克风设备   : {}", info.mic);
    println!("系统声音设备 : {} [loopback]", info.loopback);
    println!("输出目录     : {}", info.out_dir.display());
    println!("输出格式     : WAV ({} Hz, 16-bit, 双声道)", info.sample_rate);
    println!("----------------------------------------");
    println!("[y] 开始录音");
    println!("[n] 结束录音并保存");
    println!("[q] 退出程序");
    println!("========================================");

    set_idle_hint();
    out.flush().ok();
}

pub fn set_idle_hint() {
    let mut out = stdout();
    let _ = execute!(
        out,
        MoveTo(0, STATUS_ROW),
        Clear(ClearType::CurrentLine),
        Print("等待操作..."),
    );
    out.flush().ok();
}

pub fn warn(msg: &str) {
    let mut out = stdout();
    let _ = execute!(
        out,
        MoveTo(0, STATUS_ROW),
        Clear(ClearType::CurrentLine),
        Print(format!("[!] {}", msg)),
    );
    out.flush().ok();
    // 让用户看清再回到 idle
    thread::sleep(Duration::from_millis(600));
    set_idle_hint();
}

pub fn set_recording_status(elapsed: Duration, size: u64) {
    let mut out = stdout();
    let _ = execute!(
        out,
        MoveTo(0, STATUS_ROW),
        Clear(ClearType::CurrentLine),
        Print(format!(
            "● 录音中 {} | 已捕获 {} | 按 n 结束",
            format_duration(elapsed),
            format_size(size)
        )),
    );
    out.flush().ok();
}

pub fn print_stopping() {
    let mut out = stdout();
    let _ = execute!(
        out,
        MoveTo(0, STATUS_ROW),
        Clear(ClearType::CurrentLine),
        Print("■ 停止录音, 写文件..."),
    );
    out.flush().ok();
}

pub fn print_saved(path: &Path, elapsed: Duration, size: u64) {
    // 在状态行下方打印结果
    println!();
    println!("■ 录音已停止, 时长 {}", format_duration(elapsed));
    println!("✔ 已保存: {} ({})", path.display(), format_size(size));
    println!();
    set_idle_hint();
}

/// 退出前确认放弃当前录音
pub fn confirm_discard() -> bool {
    let mut out = stdout();
    let _ = execute!(
        out,
        MoveTo(0, STATUS_ROW),
        Clear(ClearType::CurrentLine),
        Print("录音未保存, 确定放弃? 按 y 确认, 按其他键取消"),
    );
    out.flush().ok();

    if crossterm::event::poll(Duration::from_secs(5)).unwrap_or(false) {
        if let Ok(Event::Key(KeyEvent {
            code: KeyCode::Char('y'),
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::NONE,
            ..
        })) = crossterm::event::read()
        {
            return true;
        }
    }
    false
}

pub fn format_duration(d: Duration) -> String {
    let total = d.as_secs();
    format!("{:02}:{:02}:{:02}", total / 3600, (total / 60) % 60, total % 60)
}

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// 轮询键, 带超时. 返回 Some(c) 表示按下了字符 c.
pub fn poll_key(timeout: Duration) -> Result<Option<char>> {
    if !crossterm::event::poll(timeout)? {
        return Ok(None);
    }
    let event = crossterm::event::read()?;

    if let Event::Key(key) = event {
        if key.kind != KeyEventKind::Press {
            return Ok(None);
        }
        match key.code {
            KeyCode::Char(c) if key.modifiers.is_empty() => return Ok(Some(c)),
            KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                return Ok(Some('\u{3}')); // 主循环视作 quit-with-confirm
            }
            _ => {}
        }
    }
    Ok(None)
}