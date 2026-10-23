/** Validates the JSON Schema vocabulary emitted by the Rust contract generator. */
interface Schema {
  $ref?: string;
  allOf?: unknown[];
  anyOf?: unknown[];
  oneOf?: unknown[];
  not?: unknown;
  if?: unknown;
  then?: unknown;
  else?: unknown;
  const?: unknown;
  enum?: unknown[];
  type?: string | string[];
  minimum?: number;
  maximum?: number;
  exclusiveMinimum?: number;
  exclusiveMaximum?: number;
  minLength?: number;
  maxLength?: number;
  pattern?: string;
  minItems?: number;
  maxItems?: number;
  uniqueItems?: boolean;
  items?: unknown;
  required?: string[];
  properties?: Record<string, unknown>;
  propertyNames?: unknown;
  additionalProperties?: unknown;
}
const object = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === "object" && !Array.isArray(value);
const equal = (left: unknown, right: unknown): boolean => {
  if (left === right) return true;
  if (Array.isArray(left) && Array.isArray(right))
    return (
      left.length === right.length &&
      left.every((item, index) => equal(item, right[index]))
    );
  if (object(left) && object(right))
    return (
      Object.keys(left).length === Object.keys(right).length &&
      Object.keys(left).every(
        (name) => Object.hasOwn(right, name) && equal(left[name], right[name]),
      )
    );
  return false;
};

export function validateContractSchema(
  definitions: Readonly<Record<string, unknown>>,
  schema: unknown,
  value: unknown,
  depth = 0,
): boolean {
  if (depth > 128) return false;
  if (typeof schema === "boolean") return schema;
  if (!object(schema)) return false;
  const rule = schema as Schema;
  const check = (child: unknown, item = value) =>
    validateContractSchema(definitions, child, item, depth + 1);
  if (rule.$ref !== undefined) {
    if (typeof rule.$ref !== "string" || !rule.$ref.startsWith("#/$defs/"))
      return false;
    const name = rule.$ref.slice(8).replaceAll("~1", "/").replaceAll("~0", "~");
    if (!Object.hasOwn(definitions, name) || !check(definitions[name]))
      return false;
  }
  if (rule.allOf && !rule.allOf.every((child: unknown) => check(child)))
    return false;
  if (rule.anyOf && !rule.anyOf.some((child: unknown) => check(child)))
    return false;
  if (
    rule.oneOf &&
    rule.oneOf.filter((child: unknown) => check(child)).length !== 1
  )
    return false;
  if (rule.not !== undefined && check(rule.not)) return false;
  if (
    rule.if !== undefined &&
    !check(check(rule.if) ? (rule.then ?? true) : (rule.else ?? true))
  )
    return false;
  if (Object.hasOwn(rule, "const") && !equal(value, rule.const)) return false;
  if (rule.enum && !rule.enum.some((item: unknown) => equal(value, item)))
    return false;
  const hasType = (kind: string): boolean => {
    switch (kind) {
      case "null":
        return value === null;
      case "boolean":
        return typeof value === "boolean";
      case "integer":
        return Number.isSafeInteger(value);
      case "number":
        return typeof value === "number" && Number.isFinite(value);
      case "string":
        return typeof value === "string";
      case "array":
        return Array.isArray(value);
      case "object":
        return object(value);
      default:
        return false;
    }
  };
  if (
    rule.type &&
    !(Array.isArray(rule.type) ? rule.type : [rule.type]).some(hasType)
  )
    return false;
  if (typeof value === "number") {
    if (rule.minimum !== undefined && value < rule.minimum) return false;
    if (rule.maximum !== undefined && value > rule.maximum) return false;
    if (rule.exclusiveMinimum !== undefined && value <= rule.exclusiveMinimum)
      return false;
    if (rule.exclusiveMaximum !== undefined && value >= rule.exclusiveMaximum)
      return false;
  }
  if (typeof value === "string") {
    const length = [...value].length;
    if (rule.minLength !== undefined && length < rule.minLength) return false;
    if (rule.maxLength !== undefined && length > rule.maxLength) return false;
    if (
      rule.pattern !== undefined &&
      !new RegExp(rule.pattern, "u").test(value)
    )
      return false;
  }
  if (Array.isArray(value)) {
    if (rule.minItems !== undefined && value.length < rule.minItems)
      return false;
    if (rule.maxItems !== undefined && value.length > rule.maxItems)
      return false;
    if (
      rule.uniqueItems &&
      value.some((item, index) =>
        value.slice(0, index).some((other) => equal(item, other)),
      )
    )
      return false;
    if (
      rule.items !== undefined &&
      !value.every((item) => check(rule.items, item))
    )
      return false;
  }
  if (object(value)) {
    if (rule.required?.some((name: string) => !Object.hasOwn(value, name)))
      return false;
    const properties = rule.properties ?? {};
    for (const [name, item] of Object.entries(value)) {
      if (rule.propertyNames !== undefined && !check(rule.propertyNames, name))
        return false;
      const child = Object.hasOwn(properties, name)
        ? properties[name]
        : (rule.additionalProperties ?? true);
      if (!check(child, item)) return false;
    }
  }
  return true;
}
