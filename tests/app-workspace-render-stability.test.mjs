import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const read = path => readFileSync(new URL("../" + path, import.meta.url), "utf8");

test("App only rerenders HubWorkspace when order inputs or stable event proxies change", () => {
  const app = read("src/App.tsx");
  const hub = read("src/HubWorkspace.tsx");
  assert.ok(hub.includes("export const MemoHubWorkspace = memo(HubWorkspace)"));
  assert.ok(app.includes('MemoHubWorkspace as HubWorkspace'));
  for (const name of [
    "workspaceNew", "workspaceSettings", "workspaceProducts",
    "workspaceSelect", "workspaceWorkflowSave", "workspaceSave",
    "workspaceCommit", "workspaceChange"
  ]) assert.ok(app.includes("const " + name + " = useEventCallback("), name);
  for (const name of [
    "onNew={workspaceNew}", "onSettings={workspaceSettings}",
    "onManageProducts={workspaceProducts}", "onSelectWorkflow={workspaceSelect}",
    "onWorkflowSave={workspaceWorkflowSave}", "onSave={workspaceSave}",
    "onSaveState={workspaceCommit}", "onChange={workspaceChange}"
  ]) assert.ok(app.includes(name), name);
  assert.ok(app.includes("model.setDraft((previous) =>"));
  assert.ok(app.includes("typeof value === \"function\" ? value(previous) : value"));
  assert.ok(app.includes("const noopDockReset = () => {}"));
});
