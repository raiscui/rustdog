# Knowledge carriers: 载体维护约定

本文件定义 rdog 仓库里**知识载体**（skill / README / specs / docs / 六文件）的
维护契约。三条约定各自配一个可执行守卫, 不要靠自觉。

## 0. 为什么有这份文件

2026-10-22 的 continuous-learning 复盘抓到一个系统性问题：同一份**过时的**坐标
换算公式同时住在 6 处载体里（`skill references/protocol.md` ×2、`skill
references/control-workflow.md`、`README.md`、`specs/code-agent-rdog-control-usage.md`、
两份 coordinate plan spec），而代码侧的权威定义一直是对的。

后果不是文档不好看：Windows 4K@200% 下这份公式**差 2 倍**，agent 按它点会点空 ——
2026-10-05 那场真机实战"连续三次点空、浪费约 20 分钟"就是这个坑。当时归因成
"agent 算错"，实际是**给 agent 的尺子本身是错的**。

三个可复用的规律：

1. 载体是**手抄**的，所以会漂。→ 约定 ①
2. 载体里的"未核实"标签会静默过期，**带着一个主动误导的公式**过期。→ 约定 ②
3. 产出（commit）不记账时，知识还在 git 里但**没人知道该去哪读**。→ 约定 ③

---

## ① 载体断言必须指向真相源

**规则**：任何写进载体的**断言类内容**——公式、坐标换算、协议常量、状态机语义、
超时默认值——必须能指回 `src/` 里的**唯一定义**。指不回去的，当现场断言处理
（写清适用条件 + 边界 + 复跑方式），不写"看起来对"的抄本。

**优先写指针，不写抄本**：能指向运行时自描述的字段（如 manifest 的
`transforms.image_to_os`），就写指针 + 公式。指针不会过期，抄本会。

### 断言清单

清单由 `scripts/check-carrier-assertions.py` 持有（脚本从 `src/` **抽取**权威值，
不硬编码），本节与脚本必须一一对应 —— 脚本会双向校验两边的清单。

| 断言 id | 内容 | 权威来源 | 必须一致的载体 |
| --- | --- | --- | --- |
| `coordinate-formula-composite` | 全桌面图：图上像素 → os-logical 换算式 | `src/screenshot.rs`（`ScreenshotTransforms::composite`） | skill `references/protocol.md`、skill `references/control-workflow.md`、`README.md`、`specs/code-agent-rdog-control-usage.md`、`specs/rdog-mouse-control-coordinate-plan.md`、`specs/rdog-multi-display-screenshot-coordinate-plan.md` |
| `coordinate-formula-window` | 窗口裁剪图：多一项 `image_rect` 平移 | `src/screenshot.rs`（`ScreenshotTransforms::window`） | skill `references/protocol.md` |
| `coordinate-deprecated-formula` | 不得把 `virtual_bounds` 当 click 空间原点 | `src/screenshot.rs`（`build_os_logical_bounds` / `build_virtual_bounds`） | 上述全部 + `specs/windows-hidpi-coordinate-fix-plan.md`、`specs/zenoh-screenshot-control-plan.md` |

### 流程

```bash
# 改动载体前: 先跑一遍, 确认基线是绿的
python3 scripts/check-carrier-assertions.py

# 改动后: 再跑一遍; 红了说明载体与代码分叉了
python3 scripts/check-carrier-assertions.py
```

**新增一条断言**：同时改脚本的 `ASSERTIONS` 表与本节表格，脚本会检查两边一致。

**反例引用是允许的**：载体里可以引用旧写法讲"不要这么用"，但该行（或其上下文
窗口）必须带明确的否定/校正标记（校正 / 不要 / 原先 / 已废弃 / 不得 …）。

### 这条守卫实测能抓什么（2026-10-22 反向验证）

守卫的能力声明必须用**注入回归**验证，不能靠"读代码觉得它覆盖了"。实测矩阵：

| 注入场景 | 结果 |
| --- | --- |
| README 的权威式退回旧单段式 | 红 ✓（覆盖检查 + 污染检查各报一条） |
| protocol.md 的窗口式被换成复合式（丢 `image_rect` 平移项） | 红 ✓ |
| coordinate plan 正文退回旧式**且**删掉「校正」注记（旧式不再是反例） | 红 ✓ |
| 正确载体 + 带「校正」注记的反例引用 | 绿 ✓（反例不算分叉） |

**第一版守卫的三个真实漏洞**（都是反向验证逼出来的，已修）：
①「指针豁免」太宽 —— 载体只要在别处提过 `transforms` 就能免检 → 改成
**给了公式就必须给对公式**；
②免责词表把 `not the` / `物理` 也算标记 —— 一行里同时写着旧公式和"virtual_bounds
不是原点"时整行被豁免 → 收窄到"明确在讲这件事本身"的措辞；
③权威串在代码里是单行 `;` 分隔、载体里分两行或只写 x 轴 → 整串比对必假阳性 →
改成**去空白后按轴做前缀匹配**。

### 已知盲区（不夸大能力）

- **只覆盖坐标换算这一族**。协议常量、超时默认值、状态机语义等断言**尚未入清单** ——
  新增断言要同时改脚本与本节表格。
- **前缀匹配对"权威式 + 多余项"不敏感**：`os_x = <权威式> + 5` 这种加法会被放过
  （前缀命中即算一致）。要更严得改成整式相等，代价是容不下"同式不同排版"。
- **免责语境是窗口级判定**（本行 + 前后各一行），所以在一段标注里引用旧式不会被抓 ——
  这是设计取舍：反例引用必须留得住。

---

## ② 进度表必须带「最后复核」列

**规则**：任何**状态表**（落地情况 / 优先级 / 欠账清单）必须有一列
「最后复核」，填**这一行的状态最后一次对着代码或真机核实过的日期**。

**为什么**：状态表会过期，而且过期时**看不出它过期了**。本轮发现
`remote-control-blocked-command-pitfalls.md` 的落地表里 P3-2（Windows 输出乱码）
与 P4（`@run`）写着"未落地 / 全仓无该 verb"，而代码里 **2026-10-07 / 10-08 就已
落地** —— 过期了 16 天。带日期后，一眼能看出这行多久没人核过。

**日期怎么填**：

- 核过 → 填核实日期（格式 `YYYY-MM-DD`）
- 没核过 → 填 `未核`，**不要**留空（留空和"刚核过"长得一样）
- 核过的内容变了 → 旧日期作废，重新核、重新填

**表级日期不够**：表头写"状态 (2026-10-22 复核)"只能说明**整表**在某天核过，
不能说明**每一行**的状态还成立。逐行填。

---

## ③ 提交即记账

**规则**：每个**工作 commit**（改了代码 / spec / skill / solution 的那些）必须
在六文件里有对应条目，并在条目末尾写明提交 sha：

```markdown
- 提交: b65680f
```

账本文件 = 六文件 + `EXPERIENCE.md`。

**为什么**：2026-10-07 ~ 10-20 期间的产出（5 个 commit + 一次跨机实战 + 一次
wayfinder 建图）**一条六文件记录都没有**。知识还在 git 里，不算丢失 —— 但
**索引缺失**：下个会话读 `task_plan.md` 会以为 10-06 之后无事发生。
账本滞后比知识缺失更隐蔽，因为它看起来"仓库挺完整的"。

### 流程

```bash
# 1. 干活 → 提交
git commit -m "..."

# 2. 在对应的 WORKLOG / notes / LATER_PLANS 条目末尾写一行
#    - 提交: <sha>          (取 sha: python3 scripts/check-bookkeeping.py --ref-only HEAD)

# 3. 把这个账本改动提交掉 (纯账本提交本身豁免, 不会自我递归)
git commit -m "chore(bookkeeping): 记账 <sha>"

# 4. 验证
python3 scripts/check-bookkeeping.py
```

### 判定规则（`scripts/check-bookkeeping.py`）

- 范围 = 当前分支上，**最近一个已被账本引用的 commit 之后**的全部 commit
- 每个 commit 要么被账本引用（sha 出现），要么**只改账本文件**（记账提交豁免）
- 否则判为"漏记账"，退出码 1

**豁免**是有限的：只改账本文件的提交豁免。功能代码提交不豁免 —— 哪怕它很小。

### sha 互引的方向

- 账本 → 提交：条目里的 `- 提交: <sha>` 指向**产出该工作的那个 commit**
- 提交 → 账本：commit 改了哪些账本文件，可由 `git show --name-only` 复原
- 注意：**不要 amend 提交去补自己的 sha**（amend 会改变 sha，引用立刻失效）。
  要记账就单独提一个纯账本提交 —— 纯账本提交豁免，所以不会自我递归。

### 这条守卫实测能抓什么（2026-10-22 反向验证）

| 场景 | 结果 |
| --- | --- |
| 抽掉 WORKLOG 里的「提交账目」表（模拟账本滞后） | 红 ✓，报出 3 个未记账 commit |
| 账目齐全 | 绿 ✓ |

**已知盲区**：

- **只查增量**：范围 = 最近一个已被账本引用的 commit 之后。账本一旦停止更新，
  范围会变大（这正是想要的），但**已经很久没记账的更早历史查不到**。
- **只验"出现"不验"对得上"**：sha 出现在任何账本文件里就算已记账，不检查那条
  记录是否真的描述了这个 commit 的内容。要更强得靠人 review。
- 账本文件集合 = 六文件 + `EXPERIENCE.md`；只改这些文件的提交豁免。

---

## 收口检查

三条约定都有守卫，收口时一并跑：

```bash
python3 scripts/check-carrier-assertions.py   # 约定 ①
python3 scripts/check-bookkeeping.py          # 约定 ③
```

约定 ② 没有脚本 —— 状态表的「最后复核」列靠 review 时人眼看。当前仓里
**进度/状态表只有 1 处**：`docs/solutions/best-practices/remote-control-blocked-command-pitfalls.md`
的「落地情况」表（17 行，已全部带「最后复核」列）。
`electron-main-process-forensics.md` 里那张是**判据表**（双通道怎么判），
不是状态表，不需要日期列。
