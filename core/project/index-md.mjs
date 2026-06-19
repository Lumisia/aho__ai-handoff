export function renderIndex({ project = 'project', changed = [], taskId } = {}) {
  const lines = [`# ${project} handoff index`, '', '## CHANGED SINCE LAST HANDOFF'];
  if (!changed.length) {
    lines.push('- (none)');
  } else {
    for (const c of changed) lines.push(`- ${c.path}  [${c.status}]`);
  }
  lines.push('', '## CURRENT TASK');
  lines.push(taskId ? `→ handoff/${taskId}/capsule.json` : '→ (none)');
  return lines.join('\n') + '\n';
}
