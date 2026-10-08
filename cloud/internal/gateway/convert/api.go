// Package convert：协议转换纯函数包（chat⇄responses、chat⇄messages）与用量截取工具
// （15_CLOUD_SERVICE.md §4.1）。网关只依赖本文件声明的签名。
//
// 签名不得变更（网关按此编码）；实现分布在同包其它文件：chatreq.go / req_*.go（请求转换）、
// chatout.go / resp_to_chat.go / msg_to_chat.go（响应转换与聚合）、usage.go（用量与错误解析）、sse.go。
package convert

import (
	"forge-cloud/internal/core"
)

// ---------- 请求转换 ----------

// ChatToResponsesOptions 控制 chat.completions → Responses 请求转换。
type ChatToResponsesOptions struct {
	// Model 为实发上游模型名（覆盖 body.model）。
	Model string
	// Instructions 非空：作为 instructions，客户端 system 消息转成 role=developer 的输入消息；
	// 为空：所有 system 消息按顺序以 "\n\n" 拼接作为 instructions。
	Instructions string
	// PromptCacheKey 非空时写 prompt_cache_key。
	PromptCacheKey string
	// Stream 写入 stream 字段。
	Stream bool
	// Store 非 nil 时写入 store 字段（Codex 订阅上游要求 false）。
	Store *bool
	// DropSampling 丢弃 temperature/top_p/max_tokens/max_completion_tokens/presence_penalty/frequency_penalty 等
	// （Codex 订阅上游的推理模型不收这些参数）。
	DropSampling bool
}

// ChatToResponsesRequest 把 chat.completions 请求体转换成 Responses 请求体：
// messages→input（user/assistant 文本与 image_url→input_image、assistant.tool_calls→function_call、
// tool 消息→function_call_output）、tools（function 扁平化）、tool_choice、parallel_tool_calls、
// reasoning_effort→reasoning.effort（附 summary:auto）、response_format→text.format。
func ChatToResponsesRequest(chatBody []byte, opts ChatToResponsesOptions) ([]byte, error) {
	return chatToResponses(chatBody, opts)
}

// ChatToMessagesOptions 控制 chat.completions → Anthropic Messages 请求转换。
type ChatToMessagesOptions struct {
	Model string
	// DefaultMaxTokens：请求未给 max_tokens/max_completion_tokens 时使用（<=0 用 8192）。
	DefaultMaxTokens int
	ThinkingMode     string
	ThinkingAlwaysOn bool
}

// ChatToMessagesRequest 把 chat.completions 请求体转换成 Anthropic Messages 请求体：
// system 消息→system；image_url（data URI/URL）→image 块；assistant.tool_calls→tool_use；
// tool 消息→user 的 tool_result（连续多条合并）；相邻同角色消息合并；tools→input_schema；
// tool_choice auto/required/none/指定函数→auto/any/none/tool；reasoning_effort→自适应 effort 或手动 thinking 预算。
func ChatToMessagesRequest(chatBody []byte, opts ChatToMessagesOptions) ([]byte, error) {
	return chatToMessages(chatBody, opts)
}

// ---------- 响应转换（流式 + 非流式聚合）----------

// UpstreamError 是上游在流内或响应体中报告的错误。
type UpstreamError struct {
	Type    string
	Code    string
	Message string
	// ResetsInSeconds > 0 表示额度重置倒计时（Codex usage_limit_reached）。
	ResetsInSeconds int
}

// ToChat 把上游事件流（Responses 或 Anthropic Messages）转换成 chat.completions 语义。
type ToChat interface {
	// Feed 消费一条上游 SSE 事件，返回应写给客户端的 chat.completion.chunk SSE 帧（可能为空）。
	// 首个有效事件会先产出 role=assistant 的开头块。
	Feed(ev Event) ([]byte, error)
	// Finish 在上游流结束后调用，返回收尾帧：finish_reason 块、（includeUsage 时）usage 块、data: [DONE]。
	Finish() []byte
	// Completion 返回聚合出的非流式 chat.completion JSON（Finish 之后调用）。
	Completion() []byte
	// Usage 返回截取到的用量（InputTokens 为不含缓存命中的计费输入）。
	Usage() core.Usage
	// Done 表示已收到终止事件（response.completed / message_stop）。
	Done() bool
	// UpstreamError 返回上游在流内报告的错误（response.failed / error 事件），无则 nil。
	UpstreamError() *UpstreamError
}

// NewResponsesToChat：Responses 事件流 → chat.completion.chunk（文本、reasoning_summary→reasoning_content、
// function_call→tool_calls 增量）。
func NewResponsesToChat(model string, includeUsage bool) ToChat {
	return newResponsesToChat(model, includeUsage)
}

// NewMessagesToChat：Anthropic Messages 事件流 → chat.completion.chunk（text、thinking→reasoning_content、
// tool_use→tool_calls 增量；stop_reason 映射 end_turn/stop_sequence→stop、tool_use→tool_calls、max_tokens→length）。
func NewMessagesToChat(model string, includeUsage bool) ToChat {
	return newMessagesToChat(model, includeUsage)
}

// ChatCompletionFromResponses 把非流式 Responses 响应体转成 chat.completion JSON。
func ChatCompletionFromResponses(respBody []byte, model string) ([]byte, core.Usage, error) {
	return chatCompletionFromResponses(respBody, model)
}

// ChatCompletionFromMessages 把非流式 Anthropic Messages 响应体转成 chat.completion JSON。
func ChatCompletionFromMessages(respBody []byte, model string) ([]byte, core.Usage, error) {
	return chatCompletionFromMessages(respBody, model)
}

// ResponsesCollector 把 Responses 事件流聚合成最终 response 对象（客户端要非流式、上游只给流式时用）。
type ResponsesCollector interface {
	Feed(ev Event)
	// Result 返回 response.completed 事件里的 response 对象 JSON；未完成返回错误。
	Result() ([]byte, error)
	Usage() core.Usage
	UpstreamError() *UpstreamError
}

func NewResponsesCollector() ResponsesCollector { return newResponsesCollector() }

// ---------- 用量截取（透传路径）----------

// UsageTap 在透传流里旁路截取用量。
type UsageTap interface {
	Feed(ev Event)
	Usage() core.Usage
}

// NewChatUsageTap：chat 流末块 usage（prompt_tokens 含缓存命中：计费输入 = prompt − cached）。
func NewChatUsageTap() UsageTap { return &chatUsageTap{} }

// NewResponsesUsageTap：response.completed.response.usage（input_tokens_details.cached_tokens 为缓存读）。
func NewResponsesUsageTap() UsageTap { return &responsesUsageTap{} }

// NewMessagesUsageTap：message_start.message.usage + message_delta.usage。
func NewMessagesUsageTap() UsageTap { return &messagesUsageTap{} }

func ChatUsageFromBody(body []byte) core.Usage       { return chatUsageFromBody(body) }
func ResponsesUsageFromBody(body []byte) core.Usage  { return responsesUsageFromBody(body) }
func MessagesUsageFromBody(body []byte) core.Usage   { return messagesUsageFromBody(body) }
func EmbeddingsUsageFromBody(body []byte) core.Usage { return embeddingsUsageFromBody(body) }

// ErrorFromBody 解析上游错误响应体（OpenAI / Anthropic / Codex 三种形状），无法解析返回 nil。
func ErrorFromBody(body []byte) *UpstreamError { return errorFromBody(body) }
