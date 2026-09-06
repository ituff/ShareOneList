# AI 聊天助手 — 需求文档（第一版：LLM 接入层 + 首页入口）

## 概述

为 ShareOneList 引入 AI 聊天助手的第一阶段能力：

1. 设置中新增 LLM 供应商管理，用户可接入任意 OpenAI 兼容接口的模型供应商。
2. 首页从空状态改造为「搜索 + 聊天」智能入口，支持流式对话和模型选择。

后续阶段（不在本文档范围）：drive catalog 检索层（站点地图）、文件上下文问答、引用卡片跳转、多轮会话持久化。

## 需求

### 需求 1：LLM 供应商配置管理

用户可以在设置中添加、编辑、删除 LLM 供应商配置。

**验收标准**

1. THE 系统 SHALL 提供添加 LLM 供应商的入口，配置项包含：显示名称、接口类型（OpenAI 兼容 / Azure OpenAI）、Base URL、API Key、模型列表。
2. THE 系统 SHALL 允许用户添加多个供应商，每个供应商可配置多个模型（model id + 显示名）。
3. WHEN 用户删除供应商时，THE 系统 SHALL 同步清除其 keyring 中的 API Key，且不可恢复。
4. IF 被删除的供应商或模型是当前默认模型，THEN THE 系统 SHALL 要求先变更默认模型（删除操作前置校验）。

### 需求 2：供应商预设

系统为常见模型供应商提供预设模板，降低配置成本。

**验收标准**

1. THE 系统 SHALL 内置以下预设：OpenAI、Azure OpenAI、DeepSeek、阿里百炼（DashScope 兼容模式）、Moonshot、智谱、Ollama（本地）、自定义。
2. WHEN 用户选择预设时，THE 系统 SHALL 自动填充 Base URL 和接口类型，用户仍可修改。
3. 预设 SHALL 仅为默认值模板，THE 系统 SHALL 允许用户修改任何字段（不设白名单限制）。

### 需求 3：默认模型

存在多个供应商/模型时，必须且只能有一个默认模型。

**验收标准**

1. WHEN 用户保存第一个含有效模型的供应商时，THE 系统 SHALL 自动将其首个模型设为默认。
2. THE 系统 SHALL 允许用户随时切换默认模型，且同一时刻有且仅有一个默认。
3. 配置 SHALL 保证 default 引用指向真实存在的供应商和模型（后端校验）。

### 需求 4：API Key 安全存储

**验收标准**

1. THE 系统 SHALL 将 API Key 存储于平台安全存储（Windows Credential Manager / macOS Keychain，复用现有 keyring 设施），禁止明文写入 JSON 配置文件。
2. THE 前端 SHALL 只能读取 Key 的存在状态与掩码（如 `sk-…abcd`），不能读取完整 Key。
3. THE 后端 SHALL 保证 API Key 不出现在任何日志输出中。

### 需求 5：连接测试

**验收标准**

1. THE 系统 SHALL 提供「测试连接」能力，向目标供应商发送一个最小请求并报告结果。
2. WHEN 测试失败时，THE 系统 SHALL 区分「网络不可达」与「鉴权失败 / 接口错误」并给出对应提示。
3. Azure OpenAI 的测试 SHALL 走 Azure 专有的路径与鉴权头（deployment 路径、`api-key` header、`api-version` query）。

### 需求 6：首页智能入口

首页从空状态改造为搜索 + 聊天双入口。

**验收标准**

1. THE 首页 SHALL 提供一个居中输入框，提供「搜索文件」与「问 AI」两种去向。
2. WHEN 用户选择「搜索文件」时，THE 系统 SHALL 在当前已登录账号的 OneDrive / 文档库中执行全局搜索并展示结果，点击结果可打开文件预览。
3. WHEN 用户选择「问 AI」时，THE 系统 SHALL 进入聊天视图。
4. IF 未配置任何可用模型，THEN 聊天入口 SHALL 显示引导卡片（跳转设置），不得报错。

### 需求 7：流式聊天与模型选择

**验收标准**

1. THE 聊天 SHALL 通过流式（SSE）逐段展示模型输出，采用 Tauri event 推送，禁止轮询。
2. THE 聊天视图 SHALL 提供模型选择器，列出所有供应商下的所有模型并标记默认项；会话内可切换。
3. WHEN 生成过程中用户点击停止时，THE 系统 SHALL 取消上游请求并保留已生成内容。
4. WHEN 请求失败时，THE 系统 SHALL 以对话内错误状态呈现（非弹窗打断），支持重试。

### 需求 8：国际化与安全提示

**验收标准**

1. 所有用户可见文案 SHALL 走 i18n（en-US / zh-CN 同步）。
2. THE 系统 SHALL 在聊天入口向用户明示「对话内容与所选文件内容将发送给所选模型服务商」。

## 范围外（后续阶段）

- drive catalog（多站点/多库检索层、访问回写自学习）
- 文件上下文注入、引用卡片跳转预览
- 聊天历史持久化（拟存 SQLite）
- RAG / 向量索引（明确不做，见 DEV_PLAN 决策）
