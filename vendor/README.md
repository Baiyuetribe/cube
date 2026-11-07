# vendor/

两个薄壳，把 `gpui-pre` / `gpui-pre-platform` 的位置转给
[gpui-fast](https://github.com/longbridge/gpui-fast)。

## 为什么需要

`gpui-component` / `gpui-base` / `gpui-kit-assets` 都精确锁定
`gpui-pre =0.3.11`。Cargo 的 `[patch.crates-io]` 只有在同名包且版本满足
约束时才会生效。

上游 gpui-fast 仓库自带 `compat/gpui-pre*` 系列，但包版本写死 `0.3.9`
（对应它们支持的那版 GPUI Kit），不满足 `=0.3.11`，patch 会被静默忽略，
实际仍在用原版 GPUI——check 能过，但优化没生效。

这里复制同一份壳并声明 0.3.11，让 patch 真正接管：

```toml
[patch.crates-io]
gpui-pre = { path = "vendor/gpui-pre-compat" }
gpui-pre-platform = { path = "vendor/gpui-pre-platform-compat" }
```

每个壳的内容就是 `pub use gpui_fast::*;`，外加 feature 转发。

## 升级步骤

1. 改 `vendor/*/Cargo.toml` 里的 `version`，与上游 gpui-component 对该
   `gpui-pre` 快照的精确约束保持一致。
2. 改两个壳里 `gpui_fast` / `gpui_fast_platform` 的依赖版本。
3. 同步 `Cargo.toml` 里 `gpui-component` / `gpui-base` / `gpui-kit-assets`
   的版本。
4. `cargo tree -p cube -i gpui-fast` 确认链路，`cargo check -p cube`。

第 4 步不能省：Cargo 在 patch 未生效时**不报错**，只会让依赖回落到原版。

## 已知差异

- `gpui-pre-macros` 没有做壳。它的路径重写（`gpui::` → `::gpui_kit::`）只在
  依赖了 `gpui-kit` 时才触发（见上游 `gpui_pre_facade_paths.rs` 里的
  `crate_name("gpui-kit")`），本仓库不依赖它，因此保持原样即可。
- 原 `wey-gu/zed` fork 的 `request_native_surface_presentation_sync` 没有等价
  API。`cube-app/src/ghostty_view.rs::request_repaint` 用 `on_next_frame`
  实现同样的「请求重绘并唤醒平台帧源」，且不要求 prepaint 阶段。
