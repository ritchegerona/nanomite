/* Plan tools - ported from src/tools/plan_tools.rs */
export const createPlanSchema = {
  type: "object",
  properties: {
    name: { type: "string", description: "Unique plan name" },
    description: { type: "string", description: "Plan description" },
    tasks: {
      type: "array",
      items: {
        type: "object",
        properties: {
          description: { type: "string" },
          depends_on: { type: "array", items: { type: "integer" } },
          tags: { type: "array", items: { type: "string" } }
        },
        required: ["description"]
      }
    }
  },
  required: ["name", "description", "tasks"],
  additionalProperties: false
};

export const getPlanSchema = {
  type: "object",
  properties: {
    name: { type: "string", description: "Plan name (optional; defaults to most recent)" }
  },
  additionalProperties: false
};

export const updateTaskSchema = {
  type: "object",
  properties: {
    task_id: { type: "integer", description: "Task ID to update" },
    status: { type: "string", description: "New status: pending, in_progress, done, blocked, cancelled" },
    description: { type: "string", description: "New description" },
    depends_on: { type: "array", items: { type: "integer" }, description: "New dependency task IDs" },
    tags: { type: "array", items: { type: "string" }, description: "New tags" }
  },
  required: ["task_id"],
  additionalProperties: false
};

export const listPlansSchema = {
  type: "object",
  properties: {},
  additionalProperties: false
};

export const deletePlanSchema = {
  type: "object",
  properties: {
    name: { type: "string", description: "Plan name to delete" },
    confirm: { type: "boolean", description: "Must be true to confirm deletion" }
  },
  required: ["name", "confirm"],
  additionalProperties: false
};