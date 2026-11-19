/* Index tools - ported from src/tools/index_tools.rs */
export const indexSymbolsSchema = {
  type: "object",
  properties: {
    force_rebuild: { type: "boolean", description: "Force rebuild the index (default: false, uses cached index)" },
    languages: { type: "array", items: { type: "string" }, description: "Specific languages to index (default: all detected)" }
  },
  additionalProperties: false
};

export const findSymbolSchema = {
  type: "object",
  properties: {
    query: { type: "string", description: "Symbol name or pattern to search for (supports * wildcard)" },
    kind: { type: "string", description: "Filter by symbol kind: function, struct, enum, trait, const, mod, etc." },
    language: { type: "string", description: "Filter by language" },
    limit: { type: "integer", description: "Max results (default 50, max 200)" }
  },
  required: ["query"],
  additionalProperties: false
};

export const findReferencesSchema = {
  type: "object",
  properties: {
    symbol: { type: "string", description: "Symbol name to find references for" },
    from_file: { type: "string", description: "Optional file to start search from (for context)" },
    limit: { type: "integer", description: "Max results (default 50, max 200)" }
  },
  required: ["symbol"],
  additionalProperties: false
};