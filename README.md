# SecureNote · 安全笔记

离线加密的跨平台个人效率应用，集**笔记**、**待办**、**密码管理**三大功能于一体。

## 特性

- 🔐 **端到端加密**：Argon2id 密钥派生 + AES-256-GCM，数据离线加密存储
- 📓 **笔记**：富文本编辑（Markdown）、图片、表格
- ☑️ **待办**：按时间创建、定时提醒（系统原生通知 + 后台常驻）
- 🔑 **密码库**：卡片式管理，点击复制、右键编辑、剪贴板自动清空
- 📦 **一键导出**：加密 JSON 备份，可上传云盘，仅本应用可还原
- 🔄 **增量同步**：UUID + 时间戳，客户端互导，冲突取最新
- 💻 **跨平台**：Windows / macOS / Linux（含统信 UOS）

## 技术栈

- **前端**：Vue 3 + TypeScript + Pinia + Tiptap
- **后端**：Rust（Tauri 2）
- **存储**：SQLite（WAL 模式），业务字段 AES-256-GCM 加密
- **加密**：Argon2id + AES-256-GCM

## 目录结构

```
securenote/
├── src/                    # 前端源码
│   ├── api.ts              # Tauri 命令桥接
│   ├── logic.ts            # 可测纯函数（同步冲突等）
│   ├── types.ts            # 类型定义
│   ├── components/         # 组件
│   ├── views/              # 页面
│   └── stores/             # Pinia store
├── src-tauri/              # Rust 后端
│   ├── src/
│   │   ├── crypto.rs       # 加密引擎
│   │   ├── store.rs        # 存储引擎
│   │   ├── vault.rs        # Vault 管理
│   │   ├── reminder.rs     # 提醒调度
│   │   ├── models.rs       # 数据模型
│   │   └── lib.rs          # 命令层
│   ├── Cargo.toml
│   └── tauri.conf.json
├── tests/                  # 前端单元测试
├── docs/                   # 文档
└── prototype/              # 产品原型
```

## 开发

```bash
# 安装依赖
npm install

# 开发模式
npm run tauri dev

# 前端测试
npm test

# 构建
npm run tauri build
```

## 打包

```bash
# Windows
npm run tauri build -- --target nsis

# macOS
npm run tauri build -- --target dmg

# Linux (AppImage)
npm run tauri build -- --target appimage
```

## 安全说明

- 主密码不落盘、不可逆、不可找回（仅能重置）
- 所有业务数据（笔记/待办/密码）均经 AES-256-GCM 加密后落盘
- 导出文件为加密包，外部无法读取
- 剪贴板自动清空、支持自动锁定
