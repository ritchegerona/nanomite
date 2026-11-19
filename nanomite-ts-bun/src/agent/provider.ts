/* Provider module - types re-exported from config.ts */
import { Role, StreamEventType as StreamEventType_imported, ChatMessage, Usage, StreamEvent, ToolSchema, newTool, Reasoning, ProviderPreset, Config, DEFAULT_PRESETS, loadConfig, saveConfig, findPreset } from "./config";

// Re-export all types for convenience (aliasing StreamEventType to avoid conflict)
export { Role, StreamEventType, ChatMessage, Usage, StreamEvent, ToolSchema, newTool, Reasoning, ProviderPreset, Config, DEFAULT_PRESETS, loadConfig, saveConfig, findPreset };

// Stream event type - use the imported one via alias
// We define a local StreamEvent interface that uses the imported StreamEventType
export interface StreamEvent {
  type: StreamEventType_imported;
  content?: string;
  reasoning_content?: string;
  tool_call?: ToolCall;
  usage?: Usage;
}

// Tool call interface
export interface ToolCall {
  id: string;
  name: string;
  arguments: string;
  index?: number;
}

// Core types re-exported
export type { Role, Reasoning, ProviderPreset, Config, DEFAULT_PRESETS };
