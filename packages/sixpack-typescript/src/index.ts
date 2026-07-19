import { spawn } from "node:child_process";

export type FieldKind = "id" | "text" | "int" | "float" | "bool";

export interface LookupDefinition {
  readonly kind: FieldKind;
  readonly unique: boolean;
}

type FieldKindFor<Value> = Value extends string
  ? "id" | "text"
  : Value extends bigint
    ? "int"
    : Value extends number
      ? "float"
      : Value extends boolean
        ? "bool"
        : never;

type FieldDefinitions<Row> = {
  readonly [Key in keyof Row]-?: FieldKindFor<Row[Key]>;
};

type LookupDefinitions<Row> = {
  readonly [Key in keyof Row]?: {
    readonly kind: FieldKindFor<Row[Key]>;
    readonly unique: boolean;
  };
};

interface TableDefinition<
  Row extends { id: string },
  Lookups extends LookupDefinitions<Row>,
> {
  readonly name: string;
  readonly fields: FieldDefinitions<Row>;
  readonly lookups: Lookups;
}

export interface Page<Row> {
  readonly rows: Row[];
  readonly nextCursor: string | null;
}

const readRequestBrand: unique symbol = Symbol("sixpack.read");
const writeRequestBrand: unique symbol = Symbol("sixpack.write");
const uniqueKeyBrand: unique symbol = Symbol("sixpack.key");
const readWireBrand: unique symbol = Symbol("sixpack.read.wire");
const readDecodeBrand: unique symbol = Symbol("sixpack.read.decode");
const writeWireBrand: unique symbol = Symbol("sixpack.write.wire");

export interface ReadRequest<Result> {
  readonly [readRequestBrand]: true;
  readonly [readWireBrand]: WireRead;
  readonly [readDecodeBrand]: (value: unknown) => Result;
}

export interface WriteRequest<TableName extends string = string> {
  readonly [writeRequestBrand]: (table: TableName) => TableName;
  readonly [writeWireBrand]: WireChange;
}

export interface ManySelector<Row> extends ReadRequest<Row[]> {
  page(limit: number): PageSelector<Row>;
}

export interface PageSelector<Row> extends ReadRequest<Page<Row>> {
  cursor(cursor: string): PageSelector<Row>;
}

export interface ScanSelector<Row> extends PageSelector<Row> {
  limit(limit: number): PageSelector<Row>;
}

type LookupValue<Row, Key> = Key extends keyof Row ? Row[Key] : never;

type ByApi<Row, Lookups extends LookupDefinitions<Row>> = {
  readonly [Key in keyof Lookups]: (
    value: LookupValue<Row, Key>,
  ) => Lookups[Key] extends { readonly unique: true }
    ? ReadRequest<Row | null>
    : ManySelector<Row>;
};

type KeyApi<
  Row,
  Lookups extends LookupDefinitions<Row>,
  TableName extends string,
> = {
  readonly [Key in keyof Lookups as Lookups[Key] extends {
    readonly unique: true;
  }
    ? Key
    : never]: (value: LookupValue<Row, Key>) => UniqueKey<TableName>;
};

export interface UniqueKey<TableName extends string = string> {
  readonly [uniqueKeyBrand]: TableName;
  readonly table: TableName;
  readonly lookup: string;
  readonly value: unknown;
  readonly kind: FieldKind;
}

export interface TableApi<
  Row extends { id: string },
  Lookups extends LookupDefinitions<Row>,
  TableName extends string = string,
> {
  readonly name: TableName;
  readonly by: ByApi<Row, Lookups>;
  readonly key: KeyApi<Row, Lookups, TableName>;
  all(): ScanSelector<Row>;
  count(): ReadRequest<bigint>;
  add(row: Row): WriteRequest<TableName>;
  set(row: Row): WriteRequest<TableName>;
  edit(
    target: UniqueKey<TableName>,
    patch: NonEmptyPatch<Omit<Row, "id">>,
  ): WriteRequest<TableName>;
  remove(target: UniqueKey<TableName>): WriteRequest<TableName>;
}

export type NonEmptyPatch<Fields> = keyof Fields extends never
  ? never
  : {
      [Key in keyof Fields]-?: Required<Pick<Fields, Key>> &
        Partial<Omit<Fields, Key>>;
    }[keyof Fields];

export interface SixpackSchema {
  readonly hash: string;
  readonly tables: Readonly<Record<string, unknown>>;
}

export interface DatabaseOptions {
  readonly root: string;
  readonly workspace: string;
  readonly schemaPath: string;
  readonly schema: SixpackSchema;
  readonly binaryPath?: string;
}

export interface WriteResult {
  readonly txId: bigint;
  readonly operation: "put" | "delete";
  readonly bytesWritten: bigint;
}

export class SixpackError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "SixpackError";
  }
}

export class Database {
  readonly #options: DatabaseOptions;

  constructor(options: DatabaseOptions) {
    this.#options = options;
  }

  async init(): Promise<void> {
    await this.#invoke({ op: "init" });
  }

  async get<Result>(request: ReadRequest<Result>): Promise<Result> {
    return request[readDecodeBrand](await this.#invoke(request[readWireBrand]));
  }

  async write<TableName extends string>(
    request: WriteRequest<TableName>,
  ): Promise<WriteResult> {
    return decodeWriteResult(
      await this.#invoke(changeOperation(request[writeWireBrand])),
    );
  }

  async writeMany<TableName extends string>(
    requests: readonly WriteRequest<TableName>[],
  ): Promise<WriteResult[]> {
    const value = await this.#invoke({
      op: "write_many",
      changes: requests.map((request) => request[writeWireBrand]),
    });
    if (!Array.isArray(value)) {
      throw new SixpackError("invalid_response", "writeMany did not return an array");
    }
    return value.map(decodeWriteResult);
  }

  async #invoke(operation: WireRead | WireOperation): Promise<unknown> {
    const binary =
      this.#options.binaryPath ?? process.env.SIXPACK_BINARY ?? "sixpack";
    const args = [
      "bridge",
      "--root",
      this.#options.root,
      "--workspace",
      this.#options.workspace,
      "--schema",
      this.#options.schemaPath,
    ];
    const request = JSON.stringify({
      schema_hash: this.#options.schema.hash,
      ...operation,
    });

    const response = await new Promise<string>((resolve, reject) => {
      const child = spawn(binary, args, { stdio: ["pipe", "pipe", "pipe"] });
      let stdout = "";
      let stderr = "";

      child.stdout.setEncoding("utf8");
      child.stderr.setEncoding("utf8");
      child.stdout.on("data", (chunk: string) => {
        stdout += chunk;
      });
      child.stderr.on("data", (chunk: string) => {
        stderr += chunk;
      });
      child.once("error", (error) => {
        reject(new SixpackError("bridge_failed", error.message));
      });
      child.once("close", (code) => {
        if (code !== 0) {
          reject(
            new SixpackError(
              "bridge_failed",
              stderr.trim() || `sixpack bridge exited with code ${code}`,
            ),
          );
          return;
        }
        resolve(stdout);
      });
      child.stdin.end(request);
    });

    let decoded: BridgeResponse;
    try {
      decoded = JSON.parse(response) as BridgeResponse;
    } catch {
      throw new SixpackError(
        "invalid_response",
        "sixpack bridge returned invalid JSON",
      );
    }
    if (!decoded.ok) {
      throw new SixpackError(decoded.error.code, decoded.error.message);
    }
    return decoded.result;
  }
}

export function createTable<Row extends { id: string }>() {
  return <
    const TableName extends string,
    Lookups extends LookupDefinitions<Row>,
  >(
    definition: TableDefinition<Row, Lookups> & { readonly name: TableName },
  ): TableApi<Row, Lookups, TableName> => {
    const decodeRow = (value: unknown): Row => {
      const row = requireObject(value, `row from ${definition.name}`);
      const decoded: Record<string, unknown> = {};
      for (const [name, kind] of Object.entries(
        definition.fields as Record<string, FieldKind>,
      )) {
        decoded[name] = decodeScalar(kind, row[name]);
      }
      return decoded as Row;
    };

    const page = (
      wire: Extract<WireRead, { op: "find" | "scan" }>,
    ): PageSelector<Row> => ({
      [readRequestBrand]: true,
      [readWireBrand]: wire,
      [readDecodeBrand]: (value) => decodePage(value, decodeRow),
      cursor: (cursorValue) => page({ ...wire, cursor: cursorValue }),
    });

    const by = Object.fromEntries(
      Object.entries(
        definition.lookups as Record<string, LookupDefinition>,
      ).map(([lookup, lookupDefinition]) => [
        lookup,
        (value: unknown) => {
          const key = encodeScalar(lookupDefinition.kind, value);
          if (lookupDefinition.unique) {
            return readRequest(
              { op: "get", table: definition.name, lookup, key },
              (result) => (result === null ? null : decodeRow(result)),
            );
          }
          const wire: Extract<WireRead, { op: "find" }> = {
            op: "find",
            table: definition.name,
            lookup,
            key,
            limit: 1_000,
          };
          return {
            ...readRequest(wire, (result) =>
              decodePage(result, decodeRow).rows,
            ),
            page: (limit: number) => page({ ...wire, limit }),
          };
        },
      ]),
    ) as ByApi<Row, Lookups>;

    const key = Object.fromEntries(
      Object.entries(
        definition.lookups as Record<string, LookupDefinition>,
      )
        .filter(([, lookup]) => lookup.unique)
        .map(([lookup, lookupDefinition]) => [
          lookup,
          (value: unknown): UniqueKey<TableName> => ({
            [uniqueKeyBrand]: definition.name,
            table: definition.name,
            lookup,
            value: encodeScalar(lookupDefinition.kind, value),
            kind: lookupDefinition.kind,
          }),
        ]),
    ) as KeyApi<Row, Lookups, TableName>;

    return {
      name: definition.name,
      by,
      key,
      all: () => {
        const wire: Extract<WireRead, { op: "scan" }> = {
          op: "scan",
          table: definition.name,
        };
        return {
          ...page(wire),
          limit: (limit: number) => page({ ...wire, limit }),
        };
      },
      count: () =>
        readRequest(
          { op: "count", table: definition.name },
          (value) => decodeUnsignedInteger(value, "count"),
        ),
      add: (row) =>
        writeRequest(definition.name, {
          kind: "add",
          table: definition.name,
          row: encodeObject(definition.fields, row),
        }),
      set: (row) =>
        writeRequest(definition.name, {
          kind: "set",
          table: definition.name,
          row: encodeObject(definition.fields, row),
        }),
      edit: (target, patch) => {
        requireTarget(definition.name, target);
        return writeRequest(definition.name, {
          kind: "edit",
          table: definition.name,
          lookup: target.lookup,
          key: target.value,
          patch: encodeObject(definition.fields, patch),
        });
      },
      remove: (target) => {
        requireTarget(definition.name, target);
        return writeRequest(definition.name, {
          kind: "remove",
          table: definition.name,
          lookup: target.lookup,
          key: target.value,
        });
      },
    };
  };
}

function requireTarget(table: string, target: UniqueKey): void {
  if (target.table !== table) {
    throw new SixpackError(
      "invalid_target",
      `key for table ${target.table} cannot target table ${table}`,
    );
  }
}

function readRequest<Result>(
  wire: WireRead,
  decode: (value: unknown) => Result,
): ReadRequest<Result> {
  return {
    [readRequestBrand]: true,
    [readWireBrand]: wire,
    [readDecodeBrand]: decode,
  };
}

function writeRequest<TableName extends string>(
  table: TableName,
  wire: WireChange,
): WriteRequest<TableName> {
  return {
    [writeRequestBrand]: () => table,
    [writeWireBrand]: wire,
  };
}

function changeOperation(change: WireChange): WireOperation {
  switch (change.kind) {
    case "add":
    case "set": {
      const { kind, ...value } = change;
      return { op: kind, ...value };
    }
    case "edit": {
      const { kind, ...value } = change;
      return { op: kind, ...value };
    }
    case "remove": {
      const { kind, ...value } = change;
      return { op: kind, ...value };
    }
  }
}

function encodeObject<Row>(
  fields: FieldDefinitions<Row>,
  input: object,
): Record<string, unknown> {
  const value = input as Record<string, unknown>;
  const encoded: Record<string, unknown> = {};
  for (const [name, fieldValue] of Object.entries(value)) {
    const kind = (fields as Record<string, FieldKind>)[name];
    if (kind === undefined) {
      throw new SixpackError("unknown_field", `unknown field ${name}`);
    }
    encoded[name] = encodeScalar(kind, fieldValue);
  }
  return encoded;
}

function encodeScalar(kind: FieldKind, value: unknown): unknown {
  switch (kind) {
    case "id":
    case "text":
      if (typeof value === "string") return value;
      break;
    case "int":
      if (typeof value === "bigint") return checkedInt64(value).toString();
      break;
    case "float":
      if (typeof value === "number" && Number.isFinite(value)) return value;
      break;
    case "bool":
      if (typeof value === "boolean") return value;
      break;
  }
  throw new SixpackError("type_mismatch", `invalid ${kind} value`);
}

function decodeScalar(kind: FieldKind, value: unknown): unknown {
  if (kind === "int") {
    return int64(requireString(value, "int"));
  }
  return encodeScalar(kind, value);
}

function decodePage<Row>(
  value: unknown,
  decodeRow: (value: unknown) => Row,
): Page<Row> {
  const page = requireObject(value, "page");
  if (!Array.isArray(page.rows)) {
    throw new SixpackError("invalid_response", "page rows must be an array");
  }
  if (page.next_cursor !== null && typeof page.next_cursor !== "string") {
    throw new SixpackError(
      "invalid_response",
      "page next_cursor must be a string or null",
    );
  }
  return {
    rows: page.rows.map(decodeRow),
    nextCursor: page.next_cursor,
  };
}

function decodeWriteResult(value: unknown): WriteResult {
  const result = requireObject(value, "write result");
  if (result.operation !== "put" && result.operation !== "delete") {
    throw new SixpackError("invalid_response", "invalid write operation");
  }
  return {
    txId: decodeUnsignedInteger(result.tx_id, "tx_id"),
    operation: result.operation,
    bytesWritten: decodeUnsignedInteger(result.bytes_written, "bytes_written"),
  };
}

const I64_MIN = -(1n << 63n);
const I64_MAX = (1n << 63n) - 1n;

export function int64(value: bigint | number | string): bigint {
  let parsed: bigint;
  if (typeof value === "bigint") {
    parsed = value;
  } else if (typeof value === "number" && Number.isSafeInteger(value)) {
    parsed = BigInt(value);
  } else if (typeof value === "string" && /^-?(0|[1-9][0-9]*)$/.test(value)) {
    parsed = BigInt(value);
  } else {
    throw new SixpackError(
      "type_mismatch",
      "int64 requires a bigint, safe integer number, or decimal integer string",
    );
  }
  return checkedInt64(parsed);
}

function checkedInt64(value: bigint): bigint {
  if (value < I64_MIN || value > I64_MAX) {
    throw new SixpackError(
      "integer_out_of_range",
      "integer is outside the signed 64-bit range",
    );
  }
  return value;
}

function decodeUnsignedInteger(value: unknown, description: string): bigint {
  const encoded = requireString(value, description);
  if (!/^(0|[1-9][0-9]*)$/.test(encoded)) {
    throw new SixpackError(
      "invalid_response",
      `${description} must be an unsigned decimal integer`,
    );
  }
  return BigInt(encoded);
}

function requireObject(
  value: unknown,
  description: string,
): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new SixpackError(
      "invalid_response",
      `${description} must be an object`,
    );
  }
  return value as Record<string, unknown>;
}

function requireString(value: unknown, description: string): string {
  if (typeof value !== "string") {
    throw new SixpackError(
      "invalid_response",
      `${description} must be a string`,
    );
  }
  return value;
}

type WireRead =
  | { readonly op: "get"; readonly table: string; readonly lookup: string; readonly key: unknown }
  | {
      readonly op: "find";
      readonly table: string;
      readonly lookup: string;
      readonly key: unknown;
      readonly limit?: number;
      readonly cursor?: string;
    }
  | {
      readonly op: "scan";
      readonly table: string;
      readonly limit?: number;
      readonly cursor?: string;
    }
  | { readonly op: "count"; readonly table: string };

type WireChange =
  | { readonly kind: "add" | "set"; readonly table: string; readonly row: Record<string, unknown> }
  | {
      readonly kind: "edit";
      readonly table: string;
      readonly lookup: string;
      readonly key: unknown;
      readonly patch: Record<string, unknown>;
    }
  | {
      readonly kind: "remove";
      readonly table: string;
      readonly lookup: string;
      readonly key: unknown;
    };

type WireOperation =
  | { readonly op: "init" }
  | ({ readonly op: "add" | "set" } & Omit<Extract<WireChange, { kind: "add" | "set" }>, "kind">)
  | ({ readonly op: "edit" } & Omit<Extract<WireChange, { kind: "edit" }>, "kind">)
  | ({ readonly op: "remove" } & Omit<Extract<WireChange, { kind: "remove" }>, "kind">)
  | { readonly op: "write_many"; readonly changes: readonly WireChange[] };

type BridgeResponse =
  | { readonly ok: true; readonly result: unknown }
  | {
      readonly ok: false;
      readonly error: { readonly code: string; readonly message: string };
    };
