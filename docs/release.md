# 发布流程（生产规范）

本文档描述「个人笔记本」（SecureNote）每次功能迭代后的标准发布流程，确保**版本号规范、数据不丢失、可正常迭代升级**。

## 一、版本号规范（语义化版本 SemVer）

格式：`主版本.次版本.修订版本`，例如 `1.1.0`

- **主版本**（major）：不兼容的大改动、数据结构变更（可能影响旧版本数据读取）
- **次版本**（minor）：新增功能、向后兼容
- **修订版本**（patch）：bug 修复、向后兼容

**版本号有三处必须同步修改**（保持一致）：

1. `src-tauri/tauri.conf.json` 的 `version`
2. `src-tauri/Cargo.toml` 的 `version`
3. `package.json` 的 `version`

> `Cargo.lock` 中的 `securenote` 版本由 cargo 在构建时自动同步，无需手改。
> 前端更新检测所需的当前版本号由 Rust 端 `get_app_version` 命令从 `CARGO_PKG_VERSION` 读取，**单一来源**，避免多处不一致。

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

- **当前版本**：`1.1.0`
- **本次变更**：新增 OneNote 批量导入、滚动备份（最多 3 份）、GitHub Release 更新检测
