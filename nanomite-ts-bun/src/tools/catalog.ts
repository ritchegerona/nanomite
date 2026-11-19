/**
 * Tool catalog - ported from src/tools/catalog.rs
 * 
 * Inventory of 37 tools with their JSON schemas.
 * Tests pin catalog names to exposed schemas.
 */

import { ToolSchema } from "./provider";

// Base 37 tools (without vpn/tor feature-gated tools)
export const toolSchemas: ToolSchema[] = [
  // Files tools
  new Tool("list_files", "List files and directories at a path inside the workspace.", {
    type: "object",
    properties: {
      path: {
        type: "string",
        description: "Workspace-relative path (defaults to the workspace root)"
      }
    },
    additionalProperties: false
  }),

  new Tool("read_file", "Read a UTF-8 text file from the workspace.", {
    type: "object",
    properties: {
      path: {
        type: "string",
        description: "Workspace-relative file path"
      },
      limit: {
        type: "integer",
        description: "Max bytes to read"
      },
      offset: {
        type: "integer",
        description: "Byte offset to start from"
      }
    },
    required: ["path"],
    additionalProperties: false
  }),

  // Search tool
  new Tool("search", "Search workspace text files for a case-insensitive substring and return matching lines with paths and line numbers.", {
    type: "object",
    properties: {
      query: {
        type: "string",
        description: "Text to search for (case-insensitive)"
      },
      path: {
        type: "string",
        description: "Workspace-relative directory to search (defaults to the workspace root)"
      }
    },
    required: ["query"],
    additionalProperties: false
  }),

  // Write file tool
  new Tool("write_file", "Write content to a file inside the workspace.", {
    type: "object",
    properties: {
      path: {
        type: "string",
        description: "Workspace-relative file path"
      },
      content: {
        type: "string",
        description: "Content to write to the file"
      },
      append: {
        type: "boolean",
        description: "Whether to append to the file (default: false)"
      }
    },
    required: ["path", "content"],
    additionalProperties: false
  }),

  // Edit file tool
  new Tool("edit_file", "Edit a file by replacing occurrences of a string with another string.", {
    type: "object",
    properties: {
      path: {
        type: "string",
        description: "Workspace-relative file path"
      },
      old_string: {
        type: "string",
        description: "String to replace"
      },
      new_string: {
        type: "string",
        description: "String to replace with"
      },
      replace_all: {
        type: "boolean",
        description: "Whether to replace all occurrences (default: false)"
      }
    },
    required: ["path", "old_string", "new_string"],
    additionalProperties: false
  }),

  // Read document tool
  new Tool("read_document", "Extract text from a document in the workspace (pdf, docx, xlsx, odt, ods, rtf, csv, plus text/markdown/json/yaml/xml/html). Image files (png, jpg, tif, bmp, webp) are recognized with OCR via tesseract. Returns the detected format, section count and bounded text.", {
    type: "object",
    properties: {
      path: {
        type: "string",
        description: "Workspace-relative document path"
      }
    },
    required: ["path"],
    additionalProperties: false
  }),

  // OCR image tool
  new Tool("ocr_image", "Recognize text in a workspace image (png, jpg, jpeg, tif, bmp, pnm, webp) with tesseract OCR and return the bounded text.", {
    type: "object",
    properties: {
      path: {
        type: "string",
        description: "Workspace-relative image path"
      },
      lang: {
        type: "string",
        description: "Tesseract language pack(s), e.g. eng, deu+eng (default: NANOMITE_OCR_LANG or eng)"
      }
    },
    required: ["path"],
    additionalProperties: false
  }),

  // Run command tool
  new Tool("run_command", "Run a shell command inside the workspace (sh -c). Use for builds, git, package managers, process inspection and any Linux tooling.", {
    type: "object",
    properties: {
      command: {
        type: "string",
        description: "Shell command line, e.g. cargo test --quiet"
      },
      cwd: {
        type: "string",
        description: "Workspace-relative working directory (defaults to the workspace root)"
      },
      timeout_ms: {
        type: "integer",
        description: "Hard cap in milliseconds (max 120000, default 60000)"
      },
      sudo_password: {
        type: "string",
        description: "Optional sudo password for commands requiring elevation. If provided, command runs via sudo -S with password piped to stdin. Never logged or stored."
      }
    },
    required: ["command"],
    additionalProperties: false
  }),

  // SSH run tool
  new Tool("ssh_run", "Run a command on a remote host over SSH (key-based auth only; BatchMode).", {
    type: "object",
    properties: {
      host: {
        type: "string",
        description: "user@host or an ssh-config alias"
      },
      command: {
        type: "string",
        description: "Command line to run on the remote host"
      },
      timeout_ms: {
        type: "integer",
        description: "Hard cap in milliseconds (max 30000)"
      }
    },
    required: ["host", "command"],
    additionalProperties: false
  }),

  // Scan ports tool
  new Tool("scan_ports", "Fast concurrent TCP connect scan of a host (pure Rust, no external deps).", {
    type: "object",
    properties: {
      host: {
        type: "string",
        description: "Target host (hostname or IP)"
      },
      ports: {
        type: "array",
        items: {
          type: "integer"
        },
        description: "Ports to scan (default: common service ports)"
      },
      timeout_ms: {
        type: "integer",
        description: "Per-connection timeout in milliseconds (default 800)"
      },
      concurrency: {
        type: "integer",
        description: "Maximum concurrent connections (default 100)"
      }
    },
    required: ["host"],
    additionalProperties: false
  }),

  // Web fetch tool
  new Tool("web_fetch", "Fetch an http(s) URL and return its readable text content (HTML stripped).", {
    type: "object",
    properties: {
      url: {
        type: "string",
        description: "Absolute http(s) URL"
      }
    },
    required: ["url"],
    additionalProperties: false
  }),

  // Web search tool
  new Tool("web_search", "Search the web (DuckDuckGo) and return titles, URLs and snippets.", {
    type: "object",
    properties: {
      query: {
        type: "string",
        description: "Search query"
      }
    },
    required: ["query"],
    additionalProperties: false
  }),

  // IP lookup tool
  new Tool("ip_lookup", "Look up a public IPv4/IPv6 address (or your own public IP) for approximate location, ISP/ASN and timezone.", {
    type: "object",
    properties: {
      ip: {
        type: "string",
        description: "Public IPv4 or IPv6 address to look up. Omit or use \"me\" for your own public IP."
      }
    },
    additionalProperties: false
  }),

  // Browse tool
  new Tool("browse", "Open a URL in a headless browser (JavaScript rendered) and return the page's readable text.", {
    type: "object",
    properties: {
      url: {
        type: "string",
        description: "Absolute http(s) URL"
      },
      wait_ms: {
        type: "integer",
        description: "Optional virtual-time budget in ms for JavaScript rendering (200-15000, default 3000)"
      }
    },
    required: ["url"],
    additionalProperties: false
  }),

  // Browse screenshot tool
  new Tool("browse_screenshot", "Open a URL in a headless browser and save a full-page screenshot PNG in the workspace.", {
    type: "object",
    properties: {
      url: {
        type: "string",
        description: "Absolute http(s) URL"
      },
      output: {
        type: "string",
        description: "Workspace-relative PNG path (defaults to screenshot-<unix time>.png)"
      }
    },
    required: ["url"],
    additionalProperties: false
  }),

  // Git tools
  new Tool("git_status", "Show git status: current branch, ahead/behind counts, staged, unstaged and untracked files.", {
    type: "object",
    properties: {
      path: {
        type: "string",
        description: "Workspace-relative path to a file or directory (defaults to workspace root)"
      }
    },
    additionalProperties: false
  }),

  new Tool("git_diff", "Show git diff of working tree or staged changes, optionally for a specific path.", {
    type: "object",
    properties: {
      path: {
        type: "string",
        description: "Workspace-relative file or directory to diff (defaults to all changes)"
      },
      staged: {
        type: "boolean",
        description: "Show staged (index) changes instead of working tree (default: false)"
      },
      include_untracked: {
        type: "boolean",
        description: "Include untracked files as new content (default: false)"
      }
    },
    additionalProperties: false
  }),

  new Tool("git_log", "Show recent git commit history with optional diffs.", {
    type: "object",
    properties: {
      limit: {
        type: "integer",
        description: "Number of commits to show (default 20, max 200)"
      },
      path: {
        type: "string",
        description: "Optional path to limit history to"
      },
      show_diff: {
        type: "boolean",
        description: "Include full diff per commit (default: false)"
      }
    },
    additionalProperties: false
  }),

  new Tool("git_commit", "Create a git commit with the given message, optionally staging specific files first.", {
    type: "object",
    properties: {
      message: {
        type: "string",
        description: "Commit message"
      },
      files: {
        type: "array",
        items: {
          type: "string"
        },
        description: "Files to stage before commit (defaults to all tracked changes)"
      },
      allow_empty: {
        type: "boolean",
        description: "Allow empty commit (default: false)"
      }
    },
    required: ["message"],
    additionalProperties: false
  }),

  new Tool("git_branch", "List, create, or delete git branches.", {
    type: "object",
    properties: {
      name: {
        type: "string",
        description: "Name of the new branch (omit to list all branches)"
      },
      start_point: {
        type: "string",
        description: "Starting commit-ish (defaults to HEAD)"
      },
      delete: {
        type: "boolean",
        description: "Delete the branch instead of creating (default: false)"
      },
      force: {
        type: "boolean",
        description: "Force delete (required if branch has unmerged changes, default: false)"
      }
    },
    additionalProperties: false
  }),

  // Run tests tool
  new Tool("run_tests", "Run project tests using cargo, npm, pytest, jest, go test, or a custom command. Returns structured pass/fail results with per-test details.", {
    type: "object",
    properties: {
      runner: {
        type: "string",
        description: "Test runner: cargo, npm, pytest, jest, go, or custom"
      },
      args: {
        type: "array",
        items: {
          type: "string"
        },
        description: "Additional arguments for the runner"
      },
      cwd: {
        type: "string",
        description: "Workspace-relative working directory (defaults to workspace root)"
      },
      timeout_ms: {
        type: "integer",
        description: "Hard cap in milliseconds (max 300000, default 120000)"
      },
      command: {
        type: "string",
        description: "For custom runner: full command line to execute"
      }
    },
    required: ["runner"],
    additionalProperties: false
  }),

  // Index symbols tool
  new Tool("index_symbols", "Build or rebuild the project symbol index for fast code navigation. Uses ctags when available, falls back to regex-based indexing.", {
    type: "object",
    properties: {
      force_rebuild: {
        type: "boolean",
        description: "Force rebuild the index (default: false, uses cached index)"
      },
      languages: {
        type: "array",
        items: {
          type: "string"
        },
        description: "Specific languages to index (default: all detected)"
      }
    },
    additionalProperties: false
  }),

  // Find symbol tool
  new Tool("find_symbol", "Search the symbol index for definitions matching a pattern. Supports wildcards (*) and filters by kind/language.", {
    type: "object",
    properties: {
      query: {
        type: "string",
        description: "Symbol name or pattern to search for (supports * wildcard)"
      },
      kind: {
        type: "string",
        description: "Filter by symbol kind: function, struct, enum, trait, const, mod, etc."
      },
      language: {
        type: "string",
        description: "Filter by language"
      },
      limit: {
        type: "integer",
        description: "Max results (default 50, max 200)",
        default: 50
      }
    },
    required: ["query"],
    additionalProperties: false
  }),

  // Find references tool
  new Tool("find_references", "Find all references to a symbol across the project.", {
    type: "object",
    properties: {
      symbol: {
        type: "string",
        description: "Symbol name to find references for"
      },
      from_file: {
        type: "string",
        description: "Optional file to start search from (for context)"
      },
      limit: {
        type: "integer",
        description: "Max results (default 50, max 200)",
        default: 50
      }
    },
    required: ["symbol"],
    additionalProperties: false
  }),

  // Create plan tool
  new Tool("create_plan", "Create a new task plan with structured tasks, dependencies, and tags. Use this to break down complex work into trackable steps.", {
    type: "object",
    properties: {
      name: {
        type: "string",
        description: "Unique plan name"
      },
      description: {
        type: "string",
        description: "Plan description"
      },
      tasks: {
        type: "array",
        items: {
          type: "object",
          properties: {
            description: {
              type: "string"
            },
            depends_on: {
              type: "array",
              items: {
                type: "integer"
              }
            },
            tags: {
              type: "array",
              items: {
                type: "string"
              }
            }
          },
          required: ["description"]
        }
      }
    },
    required: ["name", "description", "tasks"],
    additionalProperties: false
  }),

  // Get plan tool
  new Tool("get_plan", "Retrieve a plan by name, or the most recently updated plan if name is omitted.", {
    type: "object",
    properties: {
      name: {
        type: "string",
        description: "Plan name (optional; defaults to most recent)"
      }
    },
    additionalProperties: false
  }),

  // Update task tool
  new Tool("update_task", "Update a task's status, description, dependencies, or tags.", {
    type: "object",
    properties: {
      task_id: {
        type: "integer",
        description: "Task ID to update"
      },
      status: {
        type: "string",
        description: "New status: pending, in_progress, done, blocked, cancelled"
      },
      description: {
        type: "string",
        description: "New description"
      },
      depends_on: {
        type: "array",
        items: {
          type: "integer"
        },
        description: "New dependency task IDs"
      },
      tags: {
        type: "array",
        items: {
          type: "string"
        },
        description: "New tags"
      }
    },
    required: ["task_id"],
    additionalProperties: false
  }),

  // List plans tool
  new Tool("list_plans", "List all saved plans with completion summaries.", {
    type: "object",
    properties: {},
    additionalProperties: false
  }),

  // Delete plan tool
  new Tool("delete_plan", "Delete a plan by name (requires confirmation).", {
    type: "object",
    properties: {
      name: {
        type: "string",
        description: "Plan name to delete"
      },
      confirm: {
        type: "boolean",
        description: "Must be true to confirm deletion"
      }
    },
    required: ["name", "confirm"],
    additionalProperties: false
  }),

  // Memory tools
  new Tool("memory_save", "Save a fact or preference to persistent memory (tagged 'technique' for reuse).", {
    type: "object",
    properties: {
      key: {
        type: "string",
        description: "Memory key"
      },
      value: {
        type: "string",
        description: "Memory value"
      }
    },
    additionalProperties: false
  }),

  new Tool("memory_get", "Retrieve a fact from persistent memory.", {
    type: "object",
    properties: {
      key: {
        type: "string",
        description: "Memory key"
      }
    },
    additionalProperties: false
  }),

  new Tool("memory_list", "List all memory entries.", {
    type: "object",
    properties: {},
    additionalProperties: false
  }),

  // Propose self edit tool
  new Tool("propose_self_edit", "Stage a self-edit proposal for user review in /improve (nothing is applied automatically).", {
    type: "object",
    properties: {
      target: {
        type: "string",
        description: "Config or source file to edit"
      },
      new_value: {
        type: "string",
        description: "New value to propose"
      }
    },
    additionalProperties: false
  }),

  // Privacy tools (feature-gated: vpn, tor)
  // These are included when --features vpn,tor are active
];

// Export tool by name for dispatch
export function toolByName(name: string): ToolSchema | undefined {
  return toolSchemas.find(t => t.name === name);
}

// Export all tool names
export const toolNames: string[] = toolSchemas.map(t => t.name);

// Helper to get a tool schema by name
export function getToolSchema(name: string): ToolSchema | undefined {
  return toolSchemas.find(t => t.name === name);
}

// Feature-gated privacy tools (added when vpn/tor features are active)
// These are exported separately so they can be conditionally included
export const privacyToolSchemas = [
  // VPN control tool (when feature "vpn" is active)
  // new Tool("vpn_control", "Control personal WireGuard-inspired VPN (start/stop/status).", {
  //   type: "object",
  //   properties: {
  //     action: { type: "string", description: "VPN action: start, stop, status" },
  //     config_path: { type: "string", description: "Optional path to VPN config file" }
  //   },
  //   required: ["action"],
  //   additionalProperties: false
  // }),
  
  // TOR control tool (when feature "tor" is active)
  // new Tool("tor_control", "Control TOR session (new identity, status, circuit).", {
  //   type: "object",
  //   properties: {
  //     action: { type: "string", description: "TOR action: new_identity, status, circuit" },
  //     target: { type: "string", description: "Optional target for new identity" }
  //   },
  //   required: ["action"],
  //   additionalProperties: false
  // }),
];

// Export everything
export { toolSchemas, toolByName, toolNames, getToolSchema, privacyToolSchemas };