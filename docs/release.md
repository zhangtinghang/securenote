# 发布流程（生产规范）

本文档描述「个人笔记本」（SecureNote）每次功能迭代后的标准发布流程，确保**版本号规范、数据不丢失、可正常迭代升级**。

## 一、版本号规范（语义化版本 SemVer）

格式：`主版本.次版本.修订版本`，例如 `1.1.0`

- **主版本**（major）：不兼容的大改动、数据结构变更（可能影响旧版本数据读取）
- **次版本**（minor）：新增功能、向后兼容
- **修订版本**（patch）：bug 修复、向后兼容

**版本号有六处必须同步修改**（保持一致）：

1. `src-tauri/tauri.conf.json` 的 `version`
2. `src-tauri/Cargo.toml` 的 `version`
3. `package.json` 的 `version`
4. `src-tauri/Cargo.lock` 中 `securenote` 的 `version`（cargo 构建时会自动同步，也可手动）
5. `src/updater.ts` 的 `CURRENT_VERSION`
6. `src-tauri/nsis/installer.nsi` 的 `!define VERSION` / `!define VERSIONWITHBUILD` 两行

> 前端更新检测所需的当前版本号由 Rust 端 `get_app_version` 命令从 `CARGO_PKG_VERSION` 读取，**单一来源**，避免多处不一致。

### 版本发布策略（重要）

- **推送到 git 的版本保持干净**：只在功能稳定、需要让用户升级时才 bump 主版本号（如 `1.2.0` → `1.3.0`）并打 tag 发 Release，**避免频繁发版**。
- **本地迭代用小版本号区分**：开发过程中可用 `1.3.x`（patch 位）区分本地构建，但不打 tag、不推 Release。
- 攒够一批稳定功能后，再统一 bump 次版本号发正式版。

## 二、数据备份机制（保证数据不丢失）

应用采用**多层备份**：

1. **每次关闭自动备份**：退出时静默导出加密库到 `数据目录/backup/`，滚动保留**最近 3 份**（`.snvault` 加密文件）。
2. **升级前强制备份**：用户点击「前往下载」更新前，自动调用 `force_backup` 再备份一次。
3. **手动导出**：设置页「导出数据」可随时手动导出加密备份文件到任意位置。

**还原方式**：设置页「导入数据」选择 `.snvault` 备份文件还原。

**备份文件位置**（按操作系统）：
- Windows：`%APPDATA%\com.securenote.app\backup\`
- macOS：`~/Library/Application Support/com.securenote.app/backup/`
- Linux：`~/.local/share/com.securenote.app/backup/`

## 三、升级（增量更新）

**无自建服务器**，构建包发布到 **GitHub Release**：

1. 应用启动后 3 秒**静默检查** GitHub Releases API 的 latest release。
2. 发现新版本 → 弹窗显示版本号 + 更新日志 + **数据风险提示**。
3. 用户点「前往下载」→ 先 `force_backup` 备份 → 跳转浏览器下载页。
4. 用户手动下载安装包覆盖安装（NSIS 安装器保留原数据目录，不丢数据）。

**升级风险告知**：每次弹窗都会明确告知「升级前已自动备份，本应用不会主动删除数据」。

## 四、Git 管理 + Release 发布

### 首次（拿到仓库地址后）

```bash
git remote add origin <你的仓库地址>
git branch -M main
git push -u origin main
```

### 每次迭代标准流程

```bash
# 1. 改完代码后，同步三处版本号（见上文）

# 2. 构建安装包（注意：需确保 MinGW 的 windres 在 PATH 中，见下方「构建环境」）
export PATH="/c/mingw64/bin:$PATH"   # Windows 下 MinGW 工具链（windres/gcc）
npx tauri build
# 产物在 src-tauri/target/release/bundle/nsis/个人笔记本_x.y.z_x64-setup.exe

# 3. 提交源码
git add -A
git commit -m "release: v1.1.0 - 新增批量导入与升级机制"

# 4. 打 tag
git tag v1.1.0

# 5. 推送（含 tag）
git push origin main
git push origin --tags

# 6. 到 GitHub 仓库 Release 页面，基于 v1.1.0 tag 创建 Release，
#    上传安装包，填写更新日志（更新日志会被应用拉取展示）
```

### 构建环境注意事项（Windows）

本项目 Rust 工具链为 `x86_64-pc-windows-gnu`，构建时需要 **MinGW 的 `windres`（和 `gcc`）** 来编译资源文件（图标 + 版本号）。

- `windres` 位于 `C:\mingw64\bin\windres.exe`（若你的安装路径不同请对应调整）。
- 若构建报错 `NotAttempted("windres")`，说明 PATH 里没有 MinGW，先执行：
  ```bash
  export PATH="/c/mingw64/bin:$PATH"
  ```
  再重新 `npx tauri build`。
- 修改 `Cargo.toml` 的 `version` 会触发 build script 重新运行，务必确保此时 windres 可用。

### 提交信息规范

- `feat:` 新功能
- `fix:` 修复
- `refactor:` 重构
- `release:` 版本发布
- `docs:` 文档

## 五、当前版本

- **当前版本**：`1.3.0`
- **本次变更**：
  - 图片附件外置存储（大笔记支持，不压缩）
  - 数据存储目录自定义 + 历史数据迁移
  - 备份版本管理（全量 + 增量滚动备份、任意点回滚、备份管理页）
  - 导出加密包后台化（进度展示 + 系统通知）
  - 设置页布局重组、编辑器快捷键（Tab 缩进等）
  - 关闭不再自动备份（改手动 + 闲时自动备份）

## v1.3.0 起：自定义 NSIS 安装模板（注意）

`src-tauri/nsis/installer.nsi` 是自定义 NSIS 模板（基于 Tauri 2.12.5 生成产物修改），用于把**默认安装目录从中文「个人笔记本」改为英文 `securenote`**（产品显示名/开始菜单仍是中文）。

**每次升版本必须同步改模板里的 2 行**：
```
!define VERSION "x.y.z"
!define VERSIONWITHBUILD "x.y.z.0"
```
忘记改的话安装包里显示的版本号会停留在旧值（不影响应用内版本号）。

**注意事项**：
- 模板内含构建路径（MAINBINARYSRCPATH、!include），项目目录迁移后需重新从 `target/release/nsis/x64/installer.nsi` 复制生成并重做 5 处 INSTDIR 修改。
- 升级 Tauri 版本后建议重新对齐模板（tauri upgrade 后先用默认模板构建一次，取新产物复制再改）。
- 5 处安装目录修改点：`MULTIUSER_INSTALLMODE_INSTDIR "securenote"` + 4 处 `StrCpy $INSTDIR "$PROGRAMFILES...\securenote"` / `"$LOCALAPPDATA\securenote"`。
