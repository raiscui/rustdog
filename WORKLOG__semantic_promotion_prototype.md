## [2026-07-23 16:50:22] [Session ID: omx-1784512435044-92wxat] 任务名称: 语义提升与坐标 fallback prototype checkpoint

### 任务内容

- 在main之外的throwaway branch构建并交付Recording semantic promotion logic prototype。
- 覆盖click、text、shortcut、scroll、drag,以及ambiguity、dynamic page、no-AX和stale target。

### 完成过程

- 用最小负向fixture复现stale semantic target错误回退旧坐标。
- 收紧coordinate、text和shortcut门禁,新增常驻stale re-find scenario。
- 验证13个常驻scenario和4个独立负向fixture。
- 将commit `c0d2e0158df2d8bac4d37ce34dcdc7a66276b994`推到`prototype/recording-semantic-promotion`。
- 在GitHub ticket发布非resolution checkpoint,并确认ticket保持open。

### 总结感悟

- Fresh window/display geometry只证明坐标仍合法,不能证明它仍指向录制时的语义target。
- HITL verdict尚未给出,因此本记录只代表prototype checkpoint完成,不代表Wayfinder ticket已解决。

## [2026-07-23 17:07:55] [Session ID: omx-1784512435044-92wxat] 任务名称: Semantic Promotion policy正式收口

### 任务内容

- Human确认prototype policy后,将结论固化到main并完成Wayfinder resolution。

### 完成过程

- 新增正式policy,同步CONTEXT、AGENTS和Journal handoff。
- 验证两张Mermaid图、policy assertions、内部路径和scoped diff。
- 推送main commit `3de8cd6`,关闭ticket并更新map decision pointer。

### 总结感悟

- Observation ref不属于frozen Journal;正式compiler只消费canonical candidate、selector和target provenance。
- Geometry恢复与semantic target恢复是两条独立证据链,不能互相替代。
