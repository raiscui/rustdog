# PROTOTYPE — Recording semantic promotion trace lab

这是一次性 logic prototype,不是生产实现,不会合并到 `main`。

它回答一个问题:

> Recorder compiler 什么时候可以把 physical event提升为semantic action,什么时候只能使用带window/display guard的`os-logical`坐标,什么时候必须拒绝编译?

## 运行

从仓库根目录执行:

```bash
python3 prototypes/recording_semantic_promotion/prototype.py --all
```

默认输出每个gate变化后的完整内存state。快速浏览decision:

```bash
python3 prototypes/recording_semantic_promotion/prototype.py --all --summary-only
```

查看或单跑一个scenario:

```bash
python3 prototypes/recording_semantic_promotion/prototype.py --list
python3 prototypes/recording_semantic_promotion/prototype.py --scenario web-click-dynamic-refind
```

机器读取:

```bash
python3 prototypes/recording_semantic_promotion/prototype.py --all --format json
```

所有状态只存在内存并写到stdout。Prototype不写数据库、state file或artifact。

## 输入模型

`scenarios.json`保留:

- physical event数量和必要的`os-logical` point/path。
- semantic candidate数量、ownership、action capability和durable selector。
- capture-time ref,但它只作为trace provenance,绝不进入command preview。
- execution-time selector re-find结果。
- Participating Window identity、focus和geometry precondition状态。
- display topology、display guard和point/path validity。
- required/available post-action verifier。

没有浮点confidence。量化只使用可观察事实:

- candidate count。
- observation age/TTL。
- required/present guard数量。
- required/present verifier数量。
- suite decision count。

## Decision 类别

| Decision | Meaning |
| --- | --- |
| `semantic` | 使用`@web-act`、`@ax-action`、typed `TypeText`、window-targeted `@key`或`@ax-scroll` |
| `parameterized-semantic` | 文本target可恢复,但value只能由Replay Parameter提供 |
| `guarded-coordinate` | 使用`@click`、`@wheel`或`@drag`,同时要求verified window geometry、fresh rect、display guard和post-action evidence |
| `reject` | 不生成可执行step,因为target多义、stale、ownership/guard不完整或无法验证 |

## 代表场景

| Scenario | Expected |
| --- | --- |
| `native-click-unique-ax` | 唯一AXButton提升为`@ax-action AXPress` |
| `web-click-dynamic-refind` | capture ref已stale,但durable selector在fresh `AXWebArea`唯一re-find,使用`@web-act` |
| `web-click-refind-not-found` | 旧语义target执行期已丢失,即使坐标guard仍fresh也拒绝回退 |
| `web-click-ambiguous` | re-find得到两个候选,拒绝且不回退旧坐标 |
| `no-ax-click-guarded` | no-AX自绘控件,完整geometry/display/visual guard后使用`@click` |
| `text-ordinary-committed` | confirmed ordinary text生成literal typed `TypeText` |
| `text-sensitive-parameter` | 不保存value,生成parameterized typed `TypeText` |
| `text-target-unresolved` | 即使有parameter也因target unresolved而拒绝 |
| `shortcut-window-targeted` | 明确非文本chord和fresh target window生成window-targeted `@key` |
| `scroll-ax-container` | 唯一scroll container使用`@ax-scroll` |
| `scroll-no-ax-guarded` | no-AX且guard完整时使用`@wheel` |
| `drag-canvas-guarded` | 没有通用semantic drag,canvas路径用guarded `@drag` |
| `drag-stale-geometry` | window rect stale,拒绝重放drag |

## 当前 prototype policy

1. Capture-time observation ref永不持久化到Replay command。
2. Unique、owned、capability-compatible的semantic target优先。
3. Dynamic页面只允许一次bounded fresh re-find。多候选立即拒绝。
4. Text不回退raw key replay。值不可靠时参数化,target不可靠时拒绝。
5. Shortcut只在明确非文本、redaction外且target window已验证时生成。
6. Coordinate fallback只服务no-AX、free space、wheel和complex drag。
7. Ambiguous semantic target不能偷偷回退到旧坐标。
8. `performed:true`不是成功。所有可执行decision必须有fresh verifier。

## 这个 prototype 不证明什么

- 它不证明macOS CGEventTap与AX enrichment在真实时间窗口内一定关联成功。
- 它不测量真实页面candidate churn、AX latency或坐标漂移率。
- 它不定义最终`rdog.flow.v1` compiler schema。那属于后续deterministic compiler ticket。
- 它不实现Recorder、selector engine、geometry restore或control backend。

如果human接受policy,下一步应把decision写入main规格,并另开真实capture calibration只验证仍未知的时间关联率。不要把这个throwaway script改造成生产compiler。

## Human decision

本prototype推荐:

> Ambiguous或stale semantic target一律不自动回退旧坐标。只有semantic candidate为零,且window geometry、display guard、point/path和post-action verifier全部fresh时,才允许coordinate fallback。

这条规则是本ticket需要human确认的首个promotion policy决策。
