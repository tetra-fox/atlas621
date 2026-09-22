import { tableFromIPC, type Table } from "apache-arrow";

export type { Table };

export const readTable = (buffer: ArrayBuffer): Table => tableFromIPC(new Uint8Array(buffer));

const child = (table: Table, name: string) => {
  const column = table.getChild(name);
  if (!column)
    throw new Error(
      `table has no ${name} column, only ${table.schema.fields.map((f) => f.name).join(", ")}`
    );
  return column;
};

// the backing store of a primitive column, or of a fixed width list's elements, which for a
// single batch is a view on the decoded buffer rather than a copy
export const values = <T>(table: Table, name: string): T => {
  const column = child(table, name);
  const inner = column.type.children?.length ? column.getChildAt(0) : column;
  if (!inner) throw new Error(`${name} has no values`);
  return inner.toArray() as T;
};

// a list column as the offsets and the flat elements they index, the shape the callers want
export const runs = <T>(table: Table, name: string): { off: Int32Array; values: T } => {
  const column = child(table, name);
  const off = column.data[0]?.valueOffsets;
  if (!off) throw new Error(`${name} is not a list column`);
  return { off, values: values<T>(table, name) };
};

export const strings = (table: Table, name: string): { offsets: Int32Array; bytes: Uint8Array } => {
  const data = child(table, name).data[0];
  if (!data?.valueOffsets) throw new Error(`${name} is not a string column`);
  return { offsets: data.valueOffsets, bytes: data.values as Uint8Array };
};
