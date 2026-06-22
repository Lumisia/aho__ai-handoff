function typeOf(v) {
  if (Array.isArray(v)) return 'array';
  if (v === null) return 'null';
  return typeof v;
}

export function validate(value, schema, path = '$') {
  const errors = [];
  const check = (val, sch, p) => {
    if (sch.type && typeOf(val) !== sch.type) {
      errors.push(`${p}: expected ${sch.type}, got ${typeOf(val)}`);
      return;
    }
    if (sch.enum && !sch.enum.includes(val)) {
      errors.push(`${p}: ${JSON.stringify(val)} not in enum`);
    }
    if (sch.type === 'string' && typeof sch.maxLength === 'number' && val.length > sch.maxLength) {
      errors.push(`${p}: string length ${val.length} exceeds maxLength ${sch.maxLength}`);
    }
    if (sch.type === 'array' && typeof sch.maxItems === 'number' && val.length > sch.maxItems) {
      errors.push(`${p}: array length ${val.length} exceeds maxItems ${sch.maxItems}`);
    }
    if (sch.type === 'object') {
      for (const r of sch.required || []) {
        if (!(r in val)) errors.push(`${p}.${r}: required`);
      }
      for (const [k, sub] of Object.entries(sch.properties || {})) {
        if (k in val) check(val[k], sub, `${p}.${k}`);
      }
    }
    if (sch.type === 'array' && sch.items) {
      val.forEach((it, i) => check(it, sch.items, `${p}[${i}]`));
    }
  };
  check(value, schema, path);
  return { valid: errors.length === 0, errors };
}
