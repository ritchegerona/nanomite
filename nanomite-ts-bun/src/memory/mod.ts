/* Memory module - ported from src/memory/ */
import { open } from "bun:sqlite";

// Memory record structure - ported from src/memory/model.rs
export interface MemoryRecord {
  id: string;
  key: string;
  value: string;
  category: string;
  sensitivity: string;
  source: string;
  created_at: number;
  updated_at: number;
}

// Memory engine trait - ported from src/memory/store.rs
export interface MemoryEngine {
  // Search memory by query with optional filters
  search(query: string, options?: { 
    category?: string; 
    sensitivity?: string; 
    limit?: number 
  }): Promise<MemoryRecord[]>;

  // Get a specific memory by key
  get(key: string): Promise<MemoryRecord | null>;

  // Save a memory record
  save(record: Omit<MemoryRecord, "id" | "created_at" | "updated_at">): Promise<MemoryRecord>;

  // List all memories
  list(): Promise<MemoryRecord[]>;

  // Consolidate/merge duplicate memories
  consolidate(): Promise<void>;

  // Close the database connection
  close(): Promise<void>;
}

// In-memory implementation - used when NANOMITE_MEMORY=0 or as fallback
export class InMemoryEngine implements MemoryEngine {
  private store: Map<string, MemoryRecord> = new Map();
  private db: any | null = null;

  constructor(dbPath?: string) {
    if (dbPath) {
      // Open SQLite database
      this.db = open(dbPath, { 
        openFlags: 0o666 | 0o10000 // SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE
      });
      this.initializeSchema();
    }
  }

  private initializeSchema(): void {
    if (!this.db) return;
    this.db.run(`
      CREATE TABLE IF NOT EXISTS memories (
        id TEXT PRIMARY KEY,
        key TEXT NOT NULL,
        value TEXT NOT NULL,
        category TEXT DEFAULT 'general',
        sensitivity TEXT DEFAULT 'low',
        source TEXT DEFAULT 'agent',
        created_at INTEGER DEFAULT (CAST(julianday('now') AS INTEGER)),
        updated_at INTEGER DEFAULT (CAST(julianday('now') AS INTEGER))
      )
    `);

    // Create FTS5 virtual table for full-text search
    this.db.run(`
      CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(key, content='memories', content_rowid='rowid')
    `);

    // Trigger to keep FTS in sync
    this.db.run(`
      CREATE TRIGGER IF NOT EXISTS memories_fts_insert AFTER INSERT ON memories
      BEGIN
        INSERT INTO memories_fts(rowid, key, value) VALUES (new.rowid, new.key, new.value);
      END
    `);

    this.db.run(`
      CREATE TRIGGER IF NOT EXISTS memories_fts_delete AFTER DELETE ON memories
      BEGIN
        DELETE FROM memories_fts WHERE rowid = old.rowid;
      END
    `);

    this.db.run(`
      CREATE TRIGGER IF NOT EXISTS memories_fts_update AFTER UPDATE ON memories
      BEGIN
        DELETE FROM memories_fts WHERE rowid = old.rowid;
        INSERT INTO memories_fts(rowid, key, value) VALUES (new.rowid, new.key, new.value);
      END
    `);
  }

  async search(query: string, options?: { category?: string; sensitivity?: string; limit?: number }): Promise<MemoryRecord[]> {
    if (!this.db) {
      // Fallback to in-memory store
      const results: MemoryRecord[] = [];
      for (const [, record] of this.store) {
        if (record.key.toLowerCase().includes(query.toLowerCase()) ||
            record.value.toLowerCase().includes(query.toLowerCase())) {
          results.push(record);
          if (options?.limit && results.length >= options.limit) break;
        }
      }
      return results;
    }

    // Search using FTS5
    const results = this.db.query(
      `SELECT m.* FROM memories m 
       JOIN memories_fts f ON m.rowid = f.rowid 
       WHERE f MATCH ? 
       ORDER BY m.updated_at DESC`,
      [query]
    );

    const records: MemoryRecord[] = results.map((row: any) => ({
      id: row.id,
      key: row.key,
      value: row.value,
      category: row.category,
      sensitivity: row.sensitivity,
      source: row.source,
      created_at: row.created_at,
      updated_at: row.updated_at,
    }));

    if (options?.limit) {
      return records.slice(0, options.limit);
    }
    return records;
  }

  async get(key: string): Promise<MemoryRecord | null> {
    if (!this.db) {
      const record = this.store.get(key) ?? null;
      if (record) {
        return {
          id: record.id,
          key: record.key,
          value: record.value,
          category: record.category,
          sensitivity: record.sensitivity,
          source: record.source,
          created_at: record.created_at,
          updated_at: record.updated_at,
        };
      }
      return null;
    }

    const row = this.db.query(
      "SELECT * FROM memories WHERE key = ?",
      [key]
    ).at(0);

    if (!row) return null;

    return {
      id: row.id,
      key: row.key,
      value: row.value,
      category: row.category,
      sensitivity: row.sensitivity,
      source: row.source,
      created_at: row.created_at,
      updated_at: row.updated_at,
    };
  }

  async save(record: Omit<MemoryRecord, "id" | "created_at" | "updated_at">): Promise<MemoryRecord> {
    const id = crypto.randomUUID();
    const now = Math.floor(Date.now() / 1000);
    const nowDb = Math.floor(Date.now() / 1000);

    const value = record.value;
    const key = record.key;
    const category = record.category || "general";
    const sensitivity = record.sensitivity || "low";
    const source = record.source || "agent";

    if (this.db) {
      this.db.run(
        `INSERT OR REPLACE INTO memories (id, key, value, category, sensitivity, source, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
        [id, key, value, category, sensitivity, source, nowDb, nowDb]
      );
    } else {
      this.store.set(key, {
        id,
        key,
        value,
        category,
        sensitivity,
        source,
        created_at: nowDb,
        updated_at: nowDb,
      });
    }

    return {
      id,
      key,
      value,
      category,
      sensitivity,
      source,
      created_at: nowDb,
      updated_at: nowDb,
    };
  }

  async list(): Promise<MemoryRecord[]> {
    if (!this.db) {
      return Array.from(this.store.values());
    }

    const rows = this.db.query("SELECT * FROM memories ORDER BY updated_at DESC");
    return rows.map((row: any) => ({
      id: row.id,
      key: row.key,
      value: row.value,
      category: row.category,
      sensitivity: row.sensitivity,
      source: row.source,
      created_at: row.created_at,
      updated_at: row.updated_at,
    }));
  }

  async consolidate(): Promise<void> {
    // In a full implementation, this would merge duplicate memories
    // For now, no-op in the TS port
    if (this.db) {
      // Could vacuum the database here
      this.db.run("VACUUM");
    }
  }

  async close(): Promise<void> {
    if (this.db) {
      // bun:sqlite handles cleanup automatically
      this.db = null;
    }
  }
}

// Create a memory engine instance
// If dbPath is provided, use SQLite; otherwise, use in-memory store
export function createMemoryEngine(dbPath?: string): MemoryEngine {
  return new InMemoryEngine(dbPath);
}

// Memory categories - from src/memory/model.rs
export const MemoryCategories = {
  General: "general",
  Technique: "technique",
  Configuration: "configuration",
  Preference: "preference",
  Security: "security",
} as const;

export type MemoryCategory = keyof typeof MemoryCategories;

// Sensitivity levels - from src/memory/security.rs
export const MemorySensitivity = {
  Low: "low",
  Medium: "medium",
  High: "high",
  Critical: "critical",
} as const;

export type MemorySensitivityLevel = keyof typeof MemorySensitivity;