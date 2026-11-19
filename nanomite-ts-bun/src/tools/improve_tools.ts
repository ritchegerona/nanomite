/* Improve tools - ported from src/improve/ */
export const proposeSelfEditSchema = {
  type: "object",
  properties: {
    target: { type: "string", description: "Config or source file to edit" },
    new_value: { type: "string", description: "New value to propose" }
  },
  additionalProperties: false
};