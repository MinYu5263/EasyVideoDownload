# 设置功能待办

本次已完成以下四项偏好的设置界面和 SQLite 持久化，实际功能后续接入，不影响当前下载主线。

- [ ] 界面主题：按 `theme` 应用浅色/深色样式，`system` 跟随系统变化，同时适配 Element Plus。
- [ ] 下载完成通知：下载流程完成后按 `notifyOnCompletion` 发送系统通知，处理系统通知权限。
- [ ] 下载失败通知：按 `notifyOnFailure` 发送系统通知，避免重试过程中重复通知。
- [ ] 关闭行为：按 `closeAction` 处理窗口关闭按钮，支持询问、隐藏到托盘与退出；提供托盘恢复及退出入口，处理下载进行中退出、托盘不可用及
  macOS 生命周期。

数据库：`app_local_data_dir()/EasyVideoDownload/app.db`，由 Tauri 按系统解析应用本地数据目录。
默认值：主题跟随系统、两种通知开启、关闭时询问。
