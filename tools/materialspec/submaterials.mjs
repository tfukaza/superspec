/** Resolve reusable, single-parent finish presets to portable numeric controls.
 * Child materials and individual bindings may override declared parameters. */
export function resolveSubmaterials(schema, definitions) {
  const resolved = {},
    visiting = new Set();
  function resolve(id) {
    if (Object.hasOwn(resolved, id)) return resolved[id];
    if (!Object.hasOwn(definitions, id))
      throw Error("Unknown submaterial " + id);
    if (visiting.has(id)) throw Error("Submaterial inheritance cycle at " + id);
    visiting.add(id);
    const recipe = definitions[id];
    const values = recipe.extends
      ? { ...resolve(recipe.extends) }
      : Object.fromEntries(
          Object.entries(schema).map(([key, field]) => [key, field.default]),
        );
    for (const [key, value] of Object.entries(recipe.parameters ?? {})) {
      const field = schema[key];
      if (
        !field ||
        !Number.isFinite(value) ||
        value < field.min ||
        value > field.max
      )
        throw Error("Invalid parameter " + id + "." + key + " = " + value);
      values[key] = value;
    }
    visiting.delete(id);
    return (resolved[id] = Object.freeze(values));
  }
  for (const id of Object.keys(definitions)) resolve(id);
  return resolved;
}
