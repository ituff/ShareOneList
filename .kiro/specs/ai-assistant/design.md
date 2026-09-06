# AI 聊天助手 — 设计文档（第一版）

## 架构总览

遵循现有分层：Rust 后端拥有全部敏感与业务逻辑（供应商配置、API Key、HTTP 调用、SSE 解析），前端只是表现层。

```text
tauri-app/src-tauri/src/llm/
  mod.rs        # 模块声明
  config.rs     # LlmConfigManager：llm.json 持久化 + keyring 存取
  client.rs     # ProviderAdapter：OpenAI 兼容 / Azure OpenAI；chat SSE 流式；测试连接
  commands.rs   # #[tauri::command] 入口

前端：
  src/components/settings/LlmSettings.tsx      # 设置页供应商管理
  src/components/home/HomePage.tsx             # 搜索 + 聊天入口
  src/stores/llmStore.ts                       # 供应商配置状态
```

## 数据模型

### Rust `models.rs`（与 `src/lib/types.ts` 字段语义一致）

```rust
pub struct LlmModelConfig { id, model_id, display_name }
pub struct LlmProviderConfig {
    id, name,
    kind: LlmProviderKind,       // openai_compatible | azure_openai
    base_url,
    has_api_key: bool,           // 运行时由后端填充；前端不可见 key 本体
    api_version: Option<String>, // 仅 azure
    models: Vec<LlmModelConfig>,
}
pub struct LlmConfig {
    providers: Vec<LlmProviderConfig>,
    default_model: Option<LlmModelRef>,   // { provider_id, model_id }
}
```

### 持久化

- 配置文件：`app_data_dir/llm.json`（独立于 `config.json`，避免设置页整体读写时覆盖 LLM 段）。
- API Key：keyring service `shareonelist`，条目名 `llm_api_key_{provider_id}`。文件里只存 `has_api_key`。
- 损坏处理：`llm.json` 解析失败时回退默认空配置（与 `config.json` 策略一致）。

### 预设表（仅默认值模板，非白名单）

| preset | kind | baseUrl 默认值 |
|---|---|---|
| openai | openai_compatible | https://api.openai.com/v1 |
| azure-openai | azure_openai | https://{resource}.openai.azure.com |
| deepseek | openai_compatible | https://api.deepseek.com/v1 |
| dashscope | openai_compatible | https://dashscope.aliyuncs.com/compatible-mode/v1 |
| moonshot | openai_compatible | https://api.moonshot.cn/v1 |
| zhipu | openai_compatible | https://open.bigmodel.cn/api/paas/v4 |
| ollama | openai_compatible | http://localhost:11434/v1 |
| custom | openai_compatible | （空，用户填写） |

## LLM 客户端

### ProviderAdapter trait

```rust
trait ProviderAdapter {
    fn chat_url(&self, cfg: &LlmProviderConfig) -> String;
    fn auth_headers(&self, cfg: &LlmProviderConfig, key: &str) -> Vec<(String, String)>;
}
```

- `openai_compatible`：`POST {base_url}/chat/completions`，`Authorization: Bearer {key}`。
- `azure_openai`：`POST {base_url}/openai/deployments/{model_id}/chat/completions?api-version={v}`，`api-key: {key}`（model_id 即 deployment 名）。

### 流式聊天

1. `llm_chat` 命令：生成 `request_id`，spawn tokio 任务，`reqwest` POST（`stream: true`）。
2. 手动解析 SSE：按行扫描 `data:` 前缀，`[DONE]` 终止；每段通过 `app_handle.emit("llm-chat-event", payload)` 推送。
3. 事件载荷：`{ request_id, kind: "delta"|"done"|"error", delta?, message? }`。
4. 取消：`llm_chat_cancel(request_id)` 通过 `tokio_util::sync::CancellationToken` 取消上游请求；前端停止监听该 request_id。
5. 代理：reqwest 默认继承系统代理设置。

## 命令清单

| 命令 | 说明 |
|---|---|
| `get_llm_config` | 读取供应商列表（含 has_api_key、掩码）与默认模型 |
| `save_llm_provider` | 新增/更新供应商；key 非空时写入 keyring |
| `delete_llm_provider` | 删除供应商 + 清除 keyring；若是默认则报 Validation 错误 |
| `set_default_model` | 校验存在性后设置默认模型 |
| `test_llm_connection` | 发送最小 chat 请求；错误映射为网络不可达 / 鉴权失败 / 接口错误 |
| `llm_chat` | 发起流式对话（异步，事件推送） |
| `llm_chat_cancel` | 取消进行中的对话 |

前端同步更新 `src/lib/tauri.ts` 封装与 `src/lib/types.ts` 类型。

## 前端

### 设置页（LlmSettings）

- 供应商列表卡片：名称、preset、baseUrl、模型 chips、默认标记、操作（编辑/删除/设默认/测试连接）。
- 编辑对话框：preset 下拉（自动填充 kind/baseUrl）、名称、baseUrl、apiVersion（仅 azure）、模型列表编辑（model id + 显示名，可增删）、API Key 密码框（留空表示不变更，显示掩码）。
- 模型 ID 输入框带模糊匹配下拉：通过 `list_llm_models`（`GET {base}/models`）拉取供应商可用模型后仅作为候选缓存（不批量填充），输入时按 精确 < 前缀 < 子串 < 子序列 排序展示最多 12 条；支持方向键/回车/Escape，允许自由输入列表外的 id（Azure 无 /models 端点，手填 deployment 名）。
- 空状态：引导文案 + 添加按钮。

### 首页（HomePage）

- 空聊天态：居中 Logo/标题、大输入框、两个动作按钮（搜索文件 / 问 AI）、可用模型提示、安全提示文案。
- 搜索：对 authStore 中所有已登录账号并行调 `searchFiles(driveId, q, "global", cloudEnv)`，结果按账号分组列表；点击项 `openPreviewTab` 打开预览并切换到 Files 区。
- 聊天态：消息流（用户/助手气泡，纯文本保留换行）、输入框置底、模型选择器（下拉，标记默认）、流式渲染订阅 `llm-chat-event`、停止按钮、错误行内呈现 + 重试。

## 状态与事件流

```text
前端 input → llm_chat(request, messages) → Rust spawn → reqwest SSE
  ← app_handle.emit("llm-chat-event", {requestId, kind, delta})
前端 listen("llm-chat-event") → 按 requestId 过滤 → 追加到消息
```

### 系统提示词（grounding 规则）

系统提示词由后端持有（`build_system_prompt`），前端传入的 system 消息会被剥离。每次提问前，前端对当前已登录账号并行执行全局 `search_files`，取前 8 条命中（name/path/webUrl/账号名）作为 `context_files` 传入 `llm_chat`。提示词规则：

1. 优先基于用户云端文件回答，并说明依据了哪些文件；
2. 云端文件与对话中找不到所需信息时，**不得**静默使用通用知识——先询问用户是否改用互联网/公共知识回答，且此类回答须明确标注"非基于云端文件"；
3. 禁止虚构文件内容、文件名或路径；
4. 使用用户的语言回答。

这是无 RAG 时代的轻量 grounding：只有文件名级检索，不读文件内容；内容级上下文注入属于 drive catalog 后续阶段。

## 正确性属性（供测试）

1. Property: LLM 配置 round-trip —— save 后 load 语义等价（provider 顺序、模型列表、默认引用不变）。
2. Property: 默认模型引用不变量 —— 任何变更操作后，default 引用要么为 None，要么指向存在的 (provider_id, model_id)。
3. Property: 删除默认供应商被拒绝且配置不变。
4. Property: keyring 条目名生成对 provider_id 是确定且无冲突的。
5. Property: SSE 解析器对分块到达的 `data:` 行能无损重组 delta 序列。
