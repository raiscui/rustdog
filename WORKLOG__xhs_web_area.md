## [2026-07-25 01:30:24] [Session ID: omx-1784789038072-clve0o] 任务名称: 修复active-browser WebArea与bare key

### 任务内容

- 修复active browser窗口选择正确但global AX快照缺WebArea的问题。
- 降低Pi/Bonsai小模型生成key命令时的嵌套引号风险。
- 完成静态、live和下游Pi多轮回归。

### 完成过程

- 用CodeGraph和Codex direct对照确认global discovery snapshot被错误复用为页面action snapshot。
- 写RED测试后实现global消歧 -> targeted window capture,保留ambiguous/not-found和targeted失败回退。
- 为`@key`增加无空白bare payload,更新skill 1.8、reference和README。
- 相关100项nextest、fmt、cargo check通过;安装当前工作树并干净重启唯一daemon。
- live active exact find、active semantic press和reset通过;Bonsai v6正式5/5成功。

### 总结感悟

- discovery snapshot与action snapshot承担不同职责,不能因为同属AX树就合并为一个真相源。
- 协议兼容可以降低小模型生成难度,但必须用明确语法边界和完整旧语法回归保护。
