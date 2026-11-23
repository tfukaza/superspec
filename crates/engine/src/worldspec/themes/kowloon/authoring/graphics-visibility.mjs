// These bindings identify the separately generated visual stock and poster
// surfaces in the Kowloon city publisher. Mixed architectural atlases, shop
// furniture, signs, lights and material variants inherit the entity default.
const minimumByBinding = new Map([
  ["builtin:grocer-props", "low"],
  ["builtin:market-hero-props", "low"],
  ["builtin:market-trays", "low"],
  ["builtin:wall-posters", "low"],
  ["builtin:snack-packet", "medium"],
  ["builtin:restaurant-food", "medium"],
  // The blended tuft layer is the fallback under the painted lawn. Very Low has
  // neither, and shows the opaque turf with its alpha-tested tips.
  ["Soft olive grass", "low"],
]);

export const kowloonDecorationMinimum = binding => minimumByBinding.get(binding);
