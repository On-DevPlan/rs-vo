'use strict';

// 平台二进制路径解析.
//
// 查找顺序 (命中即停):
//   1. RS_VO_BINARY 环境变量 (用户强制覆盖)
//   2. optionalDependency 子包 @flowot/rs-vo-<platform>-<arch>
//   3. 本地 target/release/ (开发者运行 cargo build --release 后)
//   4. 本地 binaries/<platform>-<arch>/ (随仓库分发的版本, 可能落后)

const path = require('path');
const fs = require('fs');

const PLATFORM_KEY = `${process.platform}-${process.arch}`;
const EXE_SUFFIX = process.platform === 'win32' ? '.exe' : '';
const BINARY_NAME = `rs-vo${EXE_SUFFIX}`;
const SCOPED_PKG = `@flowot/rs-vo-${PLATFORM_KEY}`;

function getBinaryPath() {
  // 1. 环境变量覆盖
  if (process.env.RS_VO_BINARY) {
    const p = process.env.RS_VO_BINARY;
    if (!fs.existsSync(p)) {
      throw new Error(`RS_VO_BINARY 指向不存在的文件: ${p}`);
    }
    return p;
  }

  // 2. optionalDependency 子包
  try {
    const pkgJsonPath = require.resolve(`${SCOPED_PKG}/package.json`);
    const candidate = path.join(path.dirname(pkgJsonPath), 'bin', BINARY_NAME);
    if (fs.existsSync(candidate)) return candidate;
  } catch (_) {
    // 子包未安装, 跳过
  }

  // 3. 本地 cargo build 产物
  const targetPath = path.join(
    __dirname, '..', 'target', 'release', BINARY_NAME
  );
  if (fs.existsSync(targetPath)) return targetPath;

  // 4. 仓库自带的 binaries/ (随包分发的旧版本)
  const bundledPath = path.join(
    __dirname, '..', 'binaries', PLATFORM_KEY, BINARY_NAME
  );
  if (fs.existsSync(bundledPath)) return bundledPath;

  throw new Error(
    `找不到适用于 ${PLATFORM_KEY} 的 rs-vo 二进制.\n` +
    `  - Windows x64 用户: 请用 npx rs-vo, 它会自动从 npm 拉取二进制\n` +
    `  - 开发者: 请先 cargo build --release\n` +
    `  - 自定义路径: 请设置 RS_VO_BINARY 环境变量`
  );
}

module.exports = {
  getBinaryPath,
  PLATFORM_KEY,
  BINARY_NAME,
  SCOPED_PKG,
};