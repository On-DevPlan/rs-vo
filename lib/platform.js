'use strict';

// 平台二进制路径解析.
//
// 查找顺序 (命中即停):
//   1. RS_VO_BINARY 环境变量 (用户强制覆盖)
//   2. 本包随附的 binaries/<platform>-<arch>/ (npm install 时一起安装)
//   3. 本地 target/release/ (开发者运行 cargo build --release 后)

const path = require('path');
const fs = require('fs');

const PLATFORM_KEY = `${process.platform}-${process.arch}`;
const EXE_SUFFIX = process.platform === 'win32' ? '.exe' : '';
const BINARY_NAME = `rs-vo${EXE_SUFFIX}`;

function getBinaryPath() {
  // 1. 环境变量覆盖
  if (process.env.RS_VO_BINARY) {
    const p = process.env.RS_VO_BINARY;
    if (!fs.existsSync(p)) {
      throw new Error(`RS_VO_BINARY 指向不存在的文件: ${p}`);
    }
    return p;
  }

  // 2. 本包随附的 binaries/ (随仓库/随包分发)
  const bundledPath = path.join(
    __dirname, '..', 'binaries', PLATFORM_KEY, BINARY_NAME
  );
  if (fs.existsSync(bundledPath)) return bundledPath;

  // 3. 本地 cargo build 产物 (开发者路径)
  const targetPath = path.join(
    __dirname, '..', 'target', 'release', BINARY_NAME
  );
  if (fs.existsSync(targetPath)) return targetPath;

  throw new Error(
    `找不到适用于 ${PLATFORM_KEY} 的 rs-vo 二进制.\n` +
    `  - 安装 @flowot/rs-vo 后 binaries/ 应随包分发\n` +
    `  - 开发者: 请先 cargo build --release\n` +
    `  - 自定义路径: 请设置 RS_VO_BINARY 环境变量`
  );
}

module.exports = {
  getBinaryPath,
  PLATFORM_KEY,
  BINARY_NAME,
};