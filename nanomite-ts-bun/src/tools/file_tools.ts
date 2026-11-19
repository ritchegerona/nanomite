/* File tools - ported from src/tools/file_tools.rs */
export const listFilesSchema = {
  type: "object",
  properties: {
    path: { type: "string", description: "Workspace-relative path (defaults to the workspace root)" }
  },
  additionalProperties: false
};

export const readFileSchema = {
  type: "object",
  properties: {
    path: { type: "string", description: "Workspace-relative file path" },
    limit: { type: "integer", description: "Max bytes to read" },
    offset: { type: "integer", description: "Byte offset to start from" }
  },
  required: ["path"],
  additionalProperties: false
};

export const writeFileSchema = {
  type: "object",
  properties: {
    path: { type: "string", description: "Workspace-relative file path" },
    content: { type: "string", description: "Content to write to the file" },
    append: { type: "boolean", description: "Whether to append to the file (default: false)" }
  },
  required: ["path", "content"],
  additionalProperties: false
};

export const editFileSchema = {
  type: "object",
  properties: {
    path: { type: "string", description: "Workspace-relative file path" },
    old_string: { type: "string", description: "String to replace" },
    new_string: { type: "string", description: "String to replace with" },
    replace_all: { type: "boolean", description: "Whether to replace all occurrences (default: false)" }
  },
  required: ["path", "old_string", "new_string"],
  additionalProperties: false
};