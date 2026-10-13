// WAV 写入器: 直接吃 i16 样本, 不做格式转换

use anyhow::{Context, Result};
use hound::{SampleFormat, WavSpec, WavWriter};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

pub struct WavWriterHandle {
    writer: WavWriter<BufWriter<File>>,
}

impl WavWriterHandle {
    pub fn create(path: &Path, sample_rate: u32) -> Result<Self> {
        let spec = WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let writer = WavWriter::create(path, spec)
            .with_context(|| format!("无法创建 WAV 文件: {}", path.display()))?;
        Ok(Self { writer })
    }

    /// 写入一个 i16 样本
    pub fn write_sample(&mut self, s: i16) -> Result<()> {
        self.writer
            .write_sample(s)
            .context("WAV 写入失败")?;
        Ok(())
    }

    /// 落盘 + 关闭文件
    pub fn finish(self) -> Result<()> {
        self.writer.finalize().context("WAV finalize 失败")?;
        Ok(())
    }
}