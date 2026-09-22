const tokens = new Map<string, string>();

export const cssToken = (name: string): string => {
  let value = tokens.get(name);
  if (value === undefined) {
    value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    tokens.set(name, value);
  }
  return value;
};
