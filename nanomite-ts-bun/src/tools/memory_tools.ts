/* Memory tools - ported from src/tools/memory_tools.rs */
export const memorySaveSchema = {
  type: "object",
  properties: {
    key: { type: "string", description: "Memory key" },
    value: { type: "string", description: "Memory value" }
  },
  additionalProperties: false
};

export const memoryGetSchema = {
  type: "object",
  properties: {
    key: { type: "string", description: "Memory key" }
  },
  additionalProperties: false
};

export const memoryListSchema = {
  type: "object",
  properties: {},
  additionalProperties: false
};