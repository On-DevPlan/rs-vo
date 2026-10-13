# rs-vo

Windows 会议录音 CLI · WASAPI shared mode + loopback · 不抢占系统设备

**`npx @flowot/rs-vo`** 一键启动，按 `y` 录音、`n` 结束、`q` 退出。

## 安装 / 使用

```bash
# 一次性试用 (推荐)
npx @flowot/rs-vo --list-devices     # 看设备能枚举出来
npx @flowot/rs-vo                    # 启动 CLI 面板

# 全局安装
npm install -g @flowot/rs-vo
rs-vo
```

只支持 Windows x64 (WASAPI)。其他平台会被 npm 拒绝安装 (`os: ["win32"]`)。

## 工作原理

```
麦克风 (Capture) ──┐
                   ├─→ 主线程: 整数混音 → hound 写 WAV
系统声音 (loopback)─┘
```

- **共享模式** (Wasapi Shared + AUTOCONVERTPCM) — 系统替我们重采样/降混，不抢扬声器/麦克风，Zoom/Teams 照常
- **loopback** — 抓扬声器正在播放的所有声音，绕过外放路径，戴耳机时对方声音清晰
- **50ms tick** — 每个采集线程每 50ms 产出恰好 N 个 i16 样本（不足补静音），两路时间轴天然对齐
- **i16 整数混音** — `(sys + mic).clamp(-32768, 32767)`，无浮点误差

## CLI 交互

启动后看到：

```
========================================
   Win Meeting Recorder  v0.1.0
========================================
麦克风设备   : 麦克风 (Realtek Audio)
系统声音设备 : 扬声器 (Realtek Audio) [loopback]
输出目录     : D:\Users\<you>\Documents\Recordings
输出格式     : WAV (44100 Hz, 16-bit, 双声道)
----------------------------------------
[y] 开始录音
[n] 结束录音并保存
[q] 退出程序
========================================
等待操作...
```

| 按键 | 状态 | 行为 |
|---|---|---|
| `y` | 待机 | 开始录音，显示 `● 录音中 hh:mm:ss` |
| `y` | 录音中 | 提示「已在录音中」 |
| `n` | 待机 | 提示「尚未开始录音」 |
| `n` | 录音中 | 停止录音，写文件，回显保存路径 |
| `q` | 录音中 | 询问放弃确认 |
| `q` | 待机 | 直接退出 |
| `Ctrl+C` | 任意 | 优雅退出（录音中会先落盘） |

## CLI 参数

```text
rs-vo.exe [OPTIONS]

  --list-devices        列出所有输入/输出设备后退出
  --out <DIR>           指定输出目录
  --name <PREFIX>       指定文件名前缀 (默认 rec)
  --rate <HZ>           输出采样率, 44100 或 48000 (默认 44100)
  -h, --help            显示帮助
  -V, --version         显示版本
```

## 环境变量（便于脚本 / CI）

| 变量 | 作用 | 等价 CLI 参数 |
|---|---|---|
| `RS_VO_BINARY` | 强制指定二进制路径 (JS 入口查找) | — |
| `RS_VO_OUT` | 默认输出目录 | `--out` |
| `RS_VO_NAME` | 默认文件名前缀 | `--name` |

环境变量优先级低于 CLI 参数。

## 文件名与默认目录

默认输出目录：`%USERPROFILE%\Documents\Recordings` (不存在会自动创建)
默认文件名：`rec_YYYYMMDD_HHMMSS.wav`

例如：`D:\Users\alice\Documents\Recordings\rec_20261013_171530.wav`

## 验证清单

1. 戴耳机放歌 → `npx rs-vo` → 按 `y` → 确认歌声没中断、耳机里依然听得到
2. 开 Zoom 说话 → 按 `n` → 回放确认对方声音 + 你的声音都有
3. 静默期 (对方闭嘴) → 回放无爆音、无断断续续

## 从源码构建

```bash
git clone https://github.com/On-DevPlan/rs-vo.git
cd rs-vo
cargo build --release
./target/release/rs-vo.exe
```

## 项目结构

```
rs-vo/                        # 这个仓库
├── Cargo.toml                # Rust 包 (binary name = rs-vo)
├── src/                      # Rust 源代码
├── package.json              # npm 包: @flowot/rs-vo
├── bin/rs-vo.js              # JS 入口: 找二进制, spawn
├── lib/platform.js           # 二进制路径解析
├── binaries/win32-x64/       # 随包分发的二进制
└── .github/workflows/
    ├── build-rs-vo.yml       # 编译 Rust, 提交二进制回 repo, 打 tag 时建 Release
    └── fuck-npm.yml          # npm 包自动发布
```

分发链路：`push main` → `build-rs-vo.yml` 编译并把 `binaries/win32-x64/rs-vo.exe` 提交回 repo → `fuck-npm.yml` 把整个包（JS + 二进制）发到 npm → 用户 `npx @flowot/rs-vo`。

## 许可

MIT