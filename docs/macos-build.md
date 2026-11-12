# macOS 安装包构建指南

本应用基于 **Tauri 2**，其打包规则是**平台锁定**的：macOS 的 `.app` / `.dmg` 安装包**必须在 macOS 上构建**（需要 Xcode 命令行工具与 Apple 签名工具链），无法在 Windows/Linux 上交叉编译。

当前代码已具备完整的跨平台兼容性（`set_content_protection` 在非 Windows 平台有降级实现、`open_external_url` 已按 `cfg(target_os)` 分别处理 Windows/macOS/Linux），图标 `.icns` 也已就绪。因此只需在 Mac 上执行下列步骤即可出包。

## 一、前置条件（Mac 本机）

1. 安装 Rust（含 `aarch64-apple-darwin` / `x86_64-apple-darwin` 目标）
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   rustup target add aarch64-apple-darwin x86_64-apple-darwin
   ```
2. 安装 Xcode Command Line Tools（提供编译工具链）
   ```bash
   xcode-select --install
   ```
3. 安装 Node.js 18+（用于前端构建）
4. 把整个 `securenote/` 项目目录拷贝到 Mac（`node_modules`、`src-tauri/target`、`dist` 可排除，重新安装/构建）

## 二、安装依赖并出包

```bash
cd securenote
npm install
npm run tauri build
```

构建产物：
- `src-tauri/target/release/bundle/macos/个人笔记本.app`（或 `SecureNote.app`）
- `src-tauri/target/release/bundle/dmg/SecureNote_1.0.0_*.dmg`（若启用了 dmg）

## 三、通用（Universal）架构包（可选，Intel + Apple Silicon 都支持）

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm run tauri build -- --target universal-apple-darwin
```

## 四、代码签名与公证（对外分发才需要）

本地自用可跳过。对外分发需 Apple Developer 账号：

```bash
# 签名
codesign --deep --force --sign "Developer ID Application: YourName (TEAMID)" \
  "src-tauri/target/release/bundle/macos/个人笔记本.app"

# 公证
xcrun notarytool submit "xxx.dmg" --apple-id "you@example.com" \
  --team-id "TEAMID" --password "app-specific-password" --wait
```

未签名的应用在他人 Mac 上首次打开需右键 →「打开」，或执行：
```bash
xattr -cr "/Applications/个人笔记本.app"
```

## 五、当前配置说明

- `tauri.conf.json` 的 `bundle.targets` 已设为 `["nsis", "dmg", "app"]`，各平台构建时自动选取对应格式（Windows→nsis，macOS→dmg/app）。
- 若只想出 `.app` 而不出 `.dmg`，可在 Mac 上临时指定：`npm run tauri build -- --bundles app`。
