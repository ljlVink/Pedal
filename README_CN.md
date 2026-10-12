<div align="center">

<img src="crates/egui-shell/logo.svg" width="280" alt="Pedal logo" />

# Pervius

**ArkTS 反编译器**

方舟字节码（`.abc` / HAP）反编译 · pandasm 反汇编 · 归档浏览与编辑 · [pedal-abc-bridge](crates/pedal-abc-bridge/) 纯 Rust 引擎

[![Rust](https://img.shields.io/badge/Rust-2024_Edition-f74c00?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![egui](https://img.shields.io/badge/egui-0.34-1ba7f5)](https://github.com/emilk/egui)
[![Platform](https://img.shields.io/badge/Platform-Windows_·_macOS_·_Linux-8957e5)](#构建)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)</br>

[特性](#特性) · [构建](#构建) · [English](README.md)

</div>

## 致谢

- [Pervius（Bkm016）](https://github.com/Bkm016/pervius) - 本项目由其 fork 而来，至今复用着它的 UI 框架与应用外壳（egui-shell、egui patch 工作区）
- [Vineflower](https://github.com/Vineflower/vineflower) - 上游 Pervius 所驱动的 Java 反编译引擎，其 jadx 风格交互奠定了本项目的设计

## 特性

### ArkTS 反编译（鸿蒙）

直接打开鸿蒙应用包 `.hap` / `.hsp` / `.har` / `.app` 或方舟字节码 `.abc` Pedal 会把其中每个 ArkTS record 反编译为可读的伪 ArkTS（jadx 风格）。同时支持两代字节码：现代应用字节码（12.x，指令覆盖率 100%）与旧版 `0.1.0.7` 系统框架布局。ESM 模块的 import/export 会被还原，全部内容可参与全局搜索与源码导出。引擎见 [pedal-abc-bridge](crates/pedal-abc-bridge/)。

<img src="screenshots/1.png" width="600" alt="截图" />

### 源码 / 反汇编双视图

每个 ArkTS record 以 tab 打开，支持 源码 / 反汇编 切换（状态栏切换器或 `Tab` 键），pandasm 反汇编按需懒生成并带专属语法高亮。资源树中的 `.panda` 条目可直接打开同一份反汇编。任意条目还可切换到交互式 Hex 视图查看原始字节。

### 全局搜索

`Double Shift` 打开搜索面板，覆盖所有反编译出的 ArkTS 源码，支持正则和大小写敏感。结果流式返回，按 record 分组，行级高亮预览，双击跳转。索引由内存源码构建，不阻塞 UI。

<img src="screenshots/2.png" width="600" alt="截图" />

### 归档浏览

左侧资源树展示包与归档内容，支持 `hap` `hsp` `har` `app` `abc` `jar` `zip` `war` `ear`。键入即过滤（Speed Search），过滤计算在后台线程完成。条目支持就地重命名与删除。将文件拖到窗口会打开第一个拖入的归档或独立文件。最近文件列表也会同步维护。

<img src="screenshots/5.png" width="600" alt="截图" />

### 导出

- **导出反编译源码**（`Ctrl+Shift+E`）- 导出伪 ArkTS（`.ets`）到指定目录，保留包结构
- **导出 JAR**（`Ctrl+Shift+S`）/ **保存 / 覆盖源归档**（`Ctrl+S`）- 面向普通（非 ArkTS）归档：将内存中的已修改条目写回或生成新归档。ArkTS 工作区为只读，暂不支持写回（HAP 重打包）

## 构建

```bash
cargo build --release
cargo run --release
```

Linux 构建依赖：`libxkbcommon-dev libwayland-dev libwayland-egl-backend-dev libasound2-dev`。

## 许可证

[MIT](LICENSE)
