import { execFile } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import Ajv from "ajv";
import standaloneCode from "ajv/dist/standalone/index.js";
import type { AnySchema } from "ajv";
import { format, resolveConfig } from "prettier";

class DesktopContractGenerator {
  readonly #root = fileURLToPath(new URL("../../", import.meta.url));
  readonly #directory = new URL("../src/desktop/generated/", import.meta.url);
  readonly #resources = new URL("../../resources/desktop/", import.meta.url);
  readonly #exec = promisify(execFile);
  readonly #maxBuffer = 16 * 1024 * 1024;

  async run(): Promise<void> {
    const arguments_ = process.argv.slice(2);
    if (arguments_.length > 1 || (arguments_.length === 1 && arguments_[0] !== "--check")) {
      throw new Error("Usage: generate-contracts.ts [--check]");
    }
    const check = arguments_.length === 1;
    const schema = await this.#rust("schema");
    const source = [
      'import type { FromSchema, JSONSchema } from "json-schema-to-ts";',
      "class DesktopSchemaLiteral<const Definition extends JSONSchema> { constructor(readonly schema: Definition) {} }",
      `export const desktopWireSchema = new DesktopSchemaLiteral(${schema}).schema;`,
      "export type DesktopRequest = FromSchema<typeof desktopWireSchema.properties.request>;",
      "export type DesktopResponse = FromSchema<typeof desktopWireSchema.properties.response>;",
      "export type DesktopEvent = FromSchema<typeof desktopWireSchema.properties.event>;",
      'export type DesktopBootstrap = Extract<Extract<DesktopResponse["result"], {status: "success"}>["value"], {operation: "bootstrap"}>["value"];',
      'export type DesktopStreamSnapshot = Extract<Extract<DesktopResponse["result"], {status: "success"}>["value"], {operation: "stream_subscribe"}>["value"];',
      "export const desktopProtocolVersion = desktopWireSchema.properties.request.properties.protocol_version.const;",
    ].join("\n");
    const configuration = await resolveConfig(
      fileURLToPath(new URL("../.prettierrc.json", import.meta.url)),
    );
    const typescript = await format(source, { ...configuration, parser: "typescript" });
    await mkdir(this.#directory, { recursive: true });
    await this.#output("contracts.ts", typescript, check);
    await this.#output("schema.json", schema, check);
    await this.#output("fixture.json", await this.#rust("schema-fixture"), check);
    const validators = await this.#validators(this.#contract(schema));
    await mkdir(this.#resources, { recursive: true });
    await this.#output("validators.mjs", validators, check, this.#resources);
    const declarations = [
      'import type { DesktopRequest, DesktopResponse, DesktopEvent } from "@/desktop/generated/contracts";',
      "export function validateDesktopRequest(value: unknown): value is DesktopRequest;",
      "export function validateDesktopResponse(value: unknown): value is DesktopResponse;",
      "export function validateDesktopEvent(value: unknown): value is DesktopEvent;",
    ].join("\n");
    await this.#output(
      "validators.d.mts",
      await format(declarations, { ...configuration, parser: "typescript" }),
      check,
      this.#resources,
    );
  }

  async #rust(command: string): Promise<string> {
    const result = await this.#exec(
      "cargo",
      ["run", "--locked", "--quiet", "--bin", "chess-tools", "--", command],
      { cwd: this.#root, maxBuffer: this.#maxBuffer },
    );
    return result.stdout;
  }

  #contract(content: string): AnySchema {
    const schema: unknown = JSON.parse(content);
    const ajv = new Ajv({ strict: true });
    if (!ajv.validate<AnySchema>("http://json-schema.org/draft-07/schema#", schema)) {
      throw new Error("The generated schema is invalid");
    }
    return schema;
  }

  async #validators(schema: AnySchema): Promise<string> {
    if (typeof schema === "boolean" || !schema.$id) {
      throw new Error("The generated schema has no identity");
    }
    const ajv = new Ajv({ strict: true, allErrors: true, code: { source: true, esm: true } });
    ajv.addSchema(schema);
    const code = standaloneCode(ajv, {
      validateDesktopRequest: `${schema.$id}#/properties/request`,
      validateDesktopResponse: `${schema.$id}#/properties/response`,
      validateDesktopEvent: `${schema.$id}#/properties/event`,
    });
    const temporary = await mkdtemp(
      fileURLToPath(new URL("../node_modules/.desktop-validators-", import.meta.url)),
    );
    try {
      const entry = `${temporary}/standalone.mjs`;
      await writeFile(entry, code);
      const result = await this.#exec(
        "bun",
        ["build", entry, "--target", "browser", "--format", "esm", "--minify"],
        { cwd: this.#root, maxBuffer: this.#maxBuffer },
      );
      return result.stdout;
    } finally {
      await rm(temporary, { recursive: true });
    }
  }

  async #output(
    name: string,
    content: string,
    check: boolean,
    directory: URL = this.#directory,
  ): Promise<void> {
    const path = new URL(name, directory);
    if (check) {
      const existing = await readFile(path, "utf8");
      if (existing !== content) {
        throw new Error(
          `Generated desktop contract drift: ${name}; run bun run contracts:generate`,
        );
      }
    } else {
      await writeFile(path, content);
    }
  }
}

await new DesktopContractGenerator().run();
