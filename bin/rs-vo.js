#!/usr/bin/env node
'use strict';

// rs-vo CLI 入口: 找到预编译二进制, 把所有参数透传给它.
//
// 安装方式 (用户侧):
//   npx rs-vo                    # 直接运行, 自动下载 Windows x64 二进制
//   npm install -g rs-vo         # 全局安装
//
// 本地开发:
//   cargo build --release
//   node bin/rs-vo.js
//
// 自定义二进制路径 (覆盖默认查找):
//   RS_VO_BINARY=/path/to/rs-vo.exe node bin/rs-vo.js

const { spawn } = require('child_process');
const { getBinaryPath } = require('../lib/platform');

let binary;
try {
  binary = getBinaryPath();
} catch (err) {
  console.error(err.message);
  process.exit(1);
}

const args = process.argv.slice(2);
const child = spawn(binary, args, { stdio: 'inherit' });

child.on('error', (err) => {
  console.error(`rs-vo: 无法执行 ${binary}: ${err.message}`);
  process.exit(1);
});

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
  } else {
    process.exit(code || 0);
  }
});