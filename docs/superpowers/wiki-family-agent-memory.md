# Wiki：家庭 Agent 与 category 级记忆共享（方案 C：单入口 + 矩阵子 agent）

> 状态：功能已实现（分支 `feat/category-scoped-read-memory-from`，8 提交，最终审查 Ready to merge）
> 配置示例：`docs/superpowers/family-agent-config-example.toml`
> 设计文档：`docs/superpowers/specs/2026-07-05-category-scoped-read-memory-from-design.md`

## 一、问题

ZeroClaw 的跨 agent 记忆共享原本**按 agent 全有或全无**：`read_memory_from = ["steward"]` 读到 steward 的每一条记忆。家庭场景的两难——管家 agent 既存全家共享的日程档案、又存主人私密信息，要么全暴露，要么拆多个入口。此外"渠道绑死 agent"使成员和领域耦合：老人问不了学习、你问不了健康。

## 二、底层能力：category 级 `read_memory_from`

```toml
read_memory_from = ["steward:family"]          # 只读 family 类
read_memory_from = ["steward:family,events"]   # 多个类
read_memory_from = ["steward"]                 # 裸别名 = 全部类（旧行为，完全兼容）
```

语法 `alias[:cat1,cat2,...]`，第一个冒号切分；与现有 agent 别名完整匹配时按裸别名解释。类别名自由字符串、ASCII 大小写不敏感、存储侧小写归一。

| 语义规则 | 说明 |
|------|------|
| 单向只读 | A 读 B ≠ B 读 A；无跨 agent 写入机制 |
| 非传递 | A 读 B，B 读 C，A 看不到 C |
| 自身全可见 | agent 永远看到自己全部类别（category 管不了自己的行）|
| fail closed | 未知类别匹配为空；缺 agent_id 不可见；markdown 后端校验期拒绝 + 工厂运行时兜底 |
| 全读路径覆盖 | recall / recall_for_agents / get / get_for_agent / list / export / count 统一走 `entry_visible` |

## 三、架构演进：为什么落在方案 C

| | A 三账户三 agent | B 单入口+共享领域 agent | **C 单入口+矩阵子 agent（采用）** |
|---|---|---|---|
| 微信账户 | 3 个 | 1 个 | **1 个** |
| 成员×领域灵活性 | ✗ 渠道绑死 | ✅ | ✅ |
| 域内成员隐私 | ✅ 结构隔离 | ⚠️ 池化靠自觉 | ✅ **结构隔离** |
| 多轮域会话连续性 | ✅ 直连 | ⚠️ 锚在前置 | ⚠️ 同 B（固有代价）|
| 路由确定性 | ✅ 静态 | ⚠️ 每轮模型判断 | ⚠️ 同 B（固有代价）|

**A 的致命伤**：成员与领域绑死，且每个账户独立扫码/state_dir。
**B 的致命伤**（代码级验证）：全家健康数据池化在一个 agent 的自有记忆里，而 ① agent 对自己的行永远全可见（category 机制只管跨 agent）② 语义召回不按发送者分区——孩子问健康时可能召回老人病历。存储层虽有 `tenant_id` 列但 agent loop 未接线。
**C 的解法**：领域×成员拆成矩阵子 agent。关键使能点（已验证，`schema.rs:19096`）：**delegate-only agent 渠道列表可为空**——子 agent 不占微信账户，扩展只加配置块。

## 四、方案 C 拓扑

```
全家成员 ──▶ 一个微信号(wechat.family) ──▶ steward（前置）
              │ 身份识别(msg.sender) + 领域判断 → delegate
    ┌─────────┼─────────┐
health-elder health-dad tutor-kid          ← 领域×成员，无渠道
    └────── read_memory_from=["steward:family"] ──────┘
```

上下文交集的实现：**微信会话**（per-sender 历史，自动）×**用户档案**（steward 的 family 类，子 agent 只读继承）×**领域角色**（子 agent 自有 IDENTITY + 领域记忆）。交集组装发生在 delegate 时，由 steward 把"谁在问+相关背景"写进任务。

**方案 C 的一个关键反直觉决策**：steward 的 `read_memory_from = []`（不读任何子 agent）。因为 steward 面向全家对话，语义召回不分发送者——若它能读 health-elder，病历可能进孩子的对话。家长监督改为**按需 delegate + 提示词级 sender 门控**（仅管理员可发起汇报类查询）。这是 C 中唯一退化为提示词防线的点。

## 四点五、成员配对与身份认证（代码级核实）

**扫码 ≠ 成员配对**：QR 是 bot 自己登录 iLink（`wechat.rs:1748`），操作者扫一次；扫码者的 `user_id` 自动持久化为第一个授权 peer。其余成员不扫码。

**授权与身份两层分离**：
- 授权层（结构性）：渠道层白名单 `is_user_allowed(sender)`，未授权消息在进 agent 前被拦截（`wechat.rs:2260`）。
- 身份层（注册表）：`ChannelMessage.sender = from_user_id`（微信侧稳定 ID，`wechat.rs:2282`），steward 按 IDENTITY.md 成员注册表映射到档案。sender 由微信平台下发、白名单校验后放行，成员无法伪造身份（账号被盗除外）。

**配对码的坑**（`pairing.rs:151`、`wechat.rs:680`）：`/bind <码>` 成功即销毁配对码；配对守卫只在**启动时白名单为空**才创建——有 peer 后重启，`/bind` 永久失效。因此第二位成员必须在首启同一运行期内 `/bind`；第三位起走手工加白名单（其 wxid 可从 debug 日志 "ignoring unauthorized message from" 获取）。白名单支持 `"*"` 通配但家庭场景不建议。

**上线序列**：启动→管理员扫码(peer#1)→成员二 `/bind`(peer#2)→成员三发消息取日志 wxid 手工入 `external_peers`(peer#3)→三个 wxid 登记 IDENTITY.md 注册表（管理员 wxid 必须手工钉死，监督门控依据，不可自助声明）。配对成本合计：1 扫码 + 1 bind + 1 手工，优于三账户方案的 3 次扫码。

## 四点六、bot 入口的本质与成员触达（平台侧核实）

**bot = 挂了微信官方 ClawBot 插件（iLink 协议）的一个普通个人微信号**。扫码登录即为某个微信号启用该插件（官方产品，有《微信ClawBot功能使用条款》背书，微信 4.1.8.67+）。

**成员获得入口 = 和这个微信号互加好友**（成员手动搜索添加/名片分享/扫码——人际操作，与 bot API 无关；iLink API 本身不支持好友管理，但加好友本来就不由 bot 执行）。加好友后直接私聊即可，消息经白名单过滤进 steward。

**强烈建议用专用小号当 bot**：插件接管所在微信号的私聊。若绑在管理员日常号上，白名单内家人发给"你"的每条消息都会被 agent 自动应答，日常聊天与 bot 会话混杂。

**两个平台级限制**（影响家庭方案，需知）：
1. **仅私聊、无群聊**：`wechat.rs` 无任何 chatroom 处理（代码核实），家庭群不能作为入口，成员各自私聊 bot。
2. **主动消息受限（已实测确认，2026-07-11 五轮实测）**：cron→微信提醒**可用但有硬性前提**，缺一条就静默失败：
   - ① 渠道被 agent 认领（`agents.<a>.channels` 含该渠道）；
   - ② agent 在 peer group 内（`peer_groups.<g>.agents` 含该 agent——双向 opt-in）；
   - ③ **任务必须带显式 delivery 配置**（`mode="announce"` + `channel` + `to`）——CLI `cron add/add-at` **不支持**投递参数，只能走声明式 `[cron.<name>.delivery]` 或 gateway REST `POST /api/cron`；
   - ④ **收件人回复窗口活跃**：发送用的 `context_token` 只在其 inbound 消息时刷新（`wechat.rs:2242`）；窗口失效时 iLink 返回 HTTP 200 + 体内错误，**被静默吞掉**。
   实测序列：①②③齐备但窗口冷 → 无声失败；成员发一条消息刷新 token 后 → **投递成功**。
   缓解：提醒类功能要求成员保持与 bot 的日常互动；重要提醒可经管理员转达。
   **上游 bug（可报）**：`send_message_items`（`wechat.rs:1808`）只检查 HTTP 状态、不解析响应体错误码，失败被当成功且零日志；且 daemon 默认 `record!` 日志不上终端、trace 又常为 none——排障时先给服务加 `-v --log-level info`。

## 四点七、成员接入实战 Runbook（生产实例版，2026-07-11）

> 前提实况：实例白名单已非空 → 配对守卫不再创建 → `/bind` 通道**永久关闭**，唯一路径是手工白名单。抓成员 wxid 需要 debug 日志（该行日志是 DEBUG 级）。

三步流程（管理员只做前两步）：

1. **让家人能找到 bot**（微信 App 人际操作）：用管理员自己访问 bot 的同一方式给家人指路（转聊天入口/名片/ClawBot 插件分享）。系统层面无法代劳。
2. **家人各发一条消息**：不会得到回复（未授权），但 wxid 会落在 debug 日志（`ignoring unauthorized message from` + from_user_id）。
3. **运维侧**：临时 `--log-level debug` 重启 → 从日志捕获 wxid → 管理员确认身份后写入两处——`peer_groups.<g>.external_peers`（授权）+ steward IDENTITY.md 成员注册表（身份识别）→ 重启 → 全员接入后日志调回 info。

注意：身份登记必须由管理员确认，绝不因对方自称而登记；管理员 wxid 是监督门控依据，只能手工钉死。

## 五、运维硬约束（每条实测验证）

1. 绑定渠道的 agent 必须**恰好认领**该渠道（`channels = ["wechat.family"]`）；无人认领的渠道消息被**直接丢弃**，无兜底。
2. 每个 wechat 账户独立 `state_dir`（方案 C 只有一个账户，天然免疫多账户覆盖问题）。
3. `read_memory_from` 只能指向**同后端** sibling；category 形式要求 SQL/向量后端。
4. **daemon 对校验失败只 WARN 不阻断启动**——凡保护机密边界的校验规则，工厂/运行时层必须有 fail-closed 兜底（本功能已内置；开发新功能牢记）。
5. agent 改名/删除正确级联到带 category 的条目（改名逐字保留后缀文本）。
6. 新增成员/领域：复制子 agent 配置块 + 加入 steward 的 `delegates` + 更新 IDENTITY.md 路由表，三处缺一不可。

## 六、能力边界与已知代价

- **delegate 是幕后代工回传，非会话转交**：多轮域会话的连续性锚在 steward 的 per-sender 历史；子 agent 每次派发相对无状态（除非自己写记忆）。每轮多一跳延迟。
- **路由靠模型每轮判断**：派错域是概率事件。缓解：IDENTITY.md 路由表写得越明确越好。
- **家长监督是提示词防线**（见第四节），不是结构保证。
- 无跨 agent 写入：成员对子 agent 说的事不会自动进全家日程，需经 steward 记录。
- category 存在性不校验：写错类名=静默匹配为空（安全方向）。
- `SubAgentContext.allowed_agent_aliases` 只管别名可达性，category 约束不随行（当前无消费者，未来接线者须知）。

## 六点五、委派子 agent 为何拿不到工具：`runtime_profile` 的设计机制（2026-07-12 根因）

**现象**：矩阵子 agent（health-elder / tutor-kid…）被 steward 派发后，反复回"`memory_recall` 未挂载 / unknown tool"，调不出用药、化验、辅导记录。数据其实完整存在各子 agent 的记忆库里（`get`/`list` 均可见）。**这不是数据问题，是子 agent 那一跳根本没有任何工具。**

### 三层假象（为什么难查）

1. **数据在、直连能用**：`zeroclaw agent -a health-elder -m "…"` 能正常召回——顶层 agent 走的是完整工具装配路径。**直连成功会给出"已修复"的假信号，掩盖委派路径的缺陷。** 验证委派问题绝不能只用直连。
2. **steward 会把失败写成"事实"**：查不到时它把"记忆为空 / 委派被 delegation_policy 挡住"等**错误归因写进自己的 core 记忆**，并按 per-sender 会话历史短路（"第 N 次了，再试也一样"），之后**不再真正委派**。诊断前必须先清掉该发送者的会话历史 + 重启，否则复现的是"它拒绝重试"而非真实委派。
3. **`agentic` 参数是摆设**：delegate 调用里的 `{"agentic":"true"}` **被完全忽略**（`delegate.rs:1196` 不读 call 参数）。真正决定是否 agentic 的是**目标 agent 的 `runtime_profile`**。

### 机制链路（代码级，`crates/zeroclaw-runtime/src/tools/delegate.rs`）

```
steward 调 delegate{agent:"health-elder", agentic:"true", prompt}
  → execute_sync_with_admission
      let agentic = self.resolve_agentic(&agent_config.runtime_profile)   // :1196  ← 只看目标的 runtime_profile
      if agentic { execute_agentic_with_admission(...) }                  // :1262  agentic：进工具循环
      else       { 单轮 LLM，enriched prompt，无 tools_registry }          // :1277  非agentic：零工具
```

**`resolve_agentic`（`:801`）的判定**：

| 目标 agent 的 `runtime_profile` | 结果 |
|---|---|
| **空字符串（未设）** | `return false` → 非 agentic |
| 指向的 profile 不存在 | `.unwrap_or(false)` → 非 agentic |
| profile 存在但 `agentic = false` | false → 非 agentic |
| profile 存在且 `agentic = true` | **true → agentic** |

`RuntimeProfileConfig.agentic` 的字段默认是 **false**（`schema.rs:11171`）。

**只有 agentic 分支会挂工具**：`execute_agentic_with_admission`（`:2275`）里 `sub_tools`（`:2316`）由 bounded/independent 两种方式装配，再交给 `run_tool_call_loop` 的 `tools_registry`（`:2419/2428`）。非 agentic 分支根本没有 `tools_registry`——子 agent 只能吐文本，`memory_recall` 自然"不在可用列表"。

> 本次实测：加 `runtime_profile="default"`（该 profile `agentic=true`）后，`DELEGATE_TOOLS_DEBUG` 打出 `sub_tools_n=47`，含 `memory_recall,memory_store,…`，health-elder 立即正常召回。缺 `runtime_profile` 时这行日志**根本不触发**——因为压根没进 agentic 路径。

### 为什么这样设计（不是 bug，是权限边界）

关键在于 **`risk_profile` 与 `runtime_profile` 是两个正交维度**：

| 维度 | 回答的问题 | 决定 |
|---|---|---|
| `risk_profile` | 这个 agent **被允许做什么** | 工具授权、沙箱、审批、命令白名单 |
| `runtime_profile` | 这个 agent 的一轮**如何执行** | agentic 循环 vs 单轮、max_tool_iterations、上下文压缩、各类 timeout、委派深度 |

委派是否 agentic **取自目标而非调用方**，是刻意的安全设计：一个 agent 的"执行形态"由**它自己的配置**决定，调用方无权把它拉进一个它没被配置进入的、能动用工具的自主循环。否则任意 caller 都能强制任意目标进入 agentic 模式动用工具——这会击穿 agent 边界。所以 `agentic` 是**目标 `runtime_profile` 的属性**，delegate 的 `agentic` 入参只是历史遗留的装饰。

**由此产生的陷阱**：delegate-only 子 agent 常只配 `risk_profile`（"允许用 memory 工具"）却漏配 `runtime_profile`（"如何执行这一轮"）。结果是——**授权上允许、执行上永不进入挂工具的循环**：空 `runtime_profile` → 默认非 agentic → 零工具。两者缺一不可，且极易只写前者。

### 硬约束（写进部署 checklist）

- **每个会被委派的子 agent 必须同时设 `risk_profile` 和 `runtime_profile`**，且 `runtime_profile` 指向的 profile `agentic = true`。只设 `risk_profile` = 静默退化成无工具单轮。
- 顶层入口 agent（steward）本就需要 agentic（它要用 delegate/memory 工具），所以它有 `runtime_profile`；矩阵子 agent 是"纯 delegate 目标"，最容易被漏配。
- 示例配置 `family-agent-config-example.toml` 已补齐四个 agent 的 `runtime_profile` + `[runtime_profiles.default] agentic=true` + 说明注释，防后人照抄再踩。

### 诊断方法论（复用于任何"子 agent 缺工具"）

1. 先清目标发送者的会话历史 + 重启，排除 steward 的"拒绝重试/假记忆"假象。
2. 在 `execute_agentic_with_admission` 装配完 `sub_tools` 后 `eprintln!` 打印工具名单。
3. **若该行日志根本不出现** → 委派没进 agentic 路径 → 目标 `runtime_profile` 缺失或非 agentic（本例即此）。
4. 若出现但缺特定工具 → 再查 `resolve_tool_policy`（`:866`，空 `allowed_tools`→None=放行）/ `delegate_admits_with_mcp`（`Some([])`=deny-all）/ bounded 的 `parent_tools` 继承。
5. 切忌用直连 `-a <子agent>` 验证委派——它走顶层路径，会骗过你。

## 七、遗留 follow-up（非阻塞）

- get-fallback 测试对 HashMap 迭代顺序敏感（覆盖概率性，代码已验证正确）
- parse+fallback 惯用法三处重复（风格）
- `tenant_id` 列已存在但未接线——若未来接线到发送者身份，方案 B 的池化缺陷可被机制性修复，届时可重评 B/C 取舍
- 可贡献回上游 zeroclaw-labs（已确认上游无同类工作）
