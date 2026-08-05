#!/usr/bin/env node

import { appendFile, readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { pathToFileURL } from "node:url";

export function parseJsonc(source) {
	let offset = 0;

	const fail = (message) => {
		throw new Error(`${message} at offset ${offset}`);
	};

	const skipIgnored = () => {
		while (offset < source.length) {
			if (/\s/.test(source[offset])) {
				offset += 1;
				continue;
			}
			if (source.startsWith("//", offset)) {
				offset += 2;
				while (offset < source.length && source[offset] !== "\n") offset += 1;
				continue;
			}
			if (source.startsWith("/*", offset)) {
				const end = source.indexOf("*/", offset + 2);
				if (end === -1) fail("Unterminated block comment");
				offset = end + 2;
				continue;
			}
			break;
		}
	};

	const parseString = () => {
		if (source[offset] !== '"') fail("Expected string");
		const start = offset;
		offset += 1;
		while (offset < source.length) {
			if (source[offset] === "\\") {
				offset += 2;
				continue;
			}
			if (source[offset] === '"') {
				offset += 1;
				try {
					return JSON.parse(source.slice(start, offset));
				} catch {
					fail("Invalid string");
				}
			}
			offset += 1;
		}
		fail("Unterminated string");
	};

	const parseNumber = () => {
		const match = source
			.slice(offset)
			.match(/^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/);
		if (!match) fail("Invalid number");
		offset += match[0].length;
		return Number(match[0]);
	};

	const parseArray = () => {
		const values = [];
		offset += 1;
		skipIgnored();
		if (source[offset] === "]") {
			offset += 1;
			return values;
		}
		while (offset < source.length) {
			values.push(parseValue());
			skipIgnored();
			if (source[offset] === "]") {
				offset += 1;
				return values;
			}
			if (source[offset] !== ",") fail("Expected ',' or ']'");
			offset += 1;
			skipIgnored();
			if (source[offset] === "]") {
				offset += 1;
				return values;
			}
		}
		fail("Unterminated array");
	};

	const parseObject = () => {
		const value = {};
		const keys = new Set();
		offset += 1;
		skipIgnored();
		if (source[offset] === "}") {
			offset += 1;
			return value;
		}
		while (offset < source.length) {
			skipIgnored();
			const key = parseString();
			if (keys.has(key)) throw new Error(`Duplicate key '${key}'`);
			keys.add(key);
			skipIgnored();
			if (source[offset] !== ":") fail("Expected ':'");
			offset += 1;
			Object.defineProperty(value, key, {
				configurable: true,
				enumerable: true,
				value: parseValue(),
				writable: true,
			});
			skipIgnored();
			if (source[offset] === "}") {
				offset += 1;
				return value;
			}
			if (source[offset] !== ",") fail("Expected ',' or '}'");
			offset += 1;
			skipIgnored();
			if (source[offset] === "}") {
				offset += 1;
				return value;
			}
		}
		fail("Unterminated object");
	};

	const parseValue = () => {
		skipIgnored();
		if (source[offset] === "{") return parseObject();
		if (source[offset] === "[") return parseArray();
		if (source[offset] === '"') return parseString();
		if (source.startsWith("true", offset)) {
			offset += 4;
			return true;
		}
		if (source.startsWith("false", offset)) {
			offset += 5;
			return false;
		}
		if (source.startsWith("null", offset)) {
			offset += 4;
			return null;
		}
		return parseNumber();
	};

	const value = parseValue();
	skipIgnored();
	if (offset !== source.length) fail("Unexpected content");
	return value;
}

function validateAgainstSchema(value, schema, path = "$") {
	if (Object.hasOwn(schema, "const") && value !== schema.const) {
		throw new Error(`${path} must equal ${JSON.stringify(schema.const)}`);
	}

	if (schema.type === "object") {
		if (value === null || Array.isArray(value) || typeof value !== "object") {
			throw new Error(`${path} must be an object`);
		}
		for (const key of schema.required ?? []) {
			if (!Object.hasOwn(value, key))
				throw new Error(`${path}.${key} is required`);
		}
		if (schema.additionalProperties === false) {
			for (const key of Object.keys(value)) {
				if (!Object.hasOwn(schema.properties ?? {}, key)) {
					throw new Error(`${path}.${key} is not allowed`);
				}
			}
		}
		for (const [key, childSchema] of Object.entries(schema.properties ?? {})) {
			if (Object.hasOwn(value, key))
				validateAgainstSchema(value[key], childSchema, `${path}.${key}`);
		}
	} else if (schema.type === "string") {
		if (typeof value !== "string") throw new Error(`${path} must be a string`);
		if (schema.pattern && !new RegExp(schema.pattern, "u").test(value)) {
			throw new Error(`${path} does not match ${schema.pattern}`);
		}
	} else if (schema.type === "integer") {
		if (!Number.isInteger(value)) throw new Error(`${path} must be an integer`);
		if (schema.minimum !== undefined && value < schema.minimum) {
			throw new Error(`${path} must be at least ${schema.minimum}`);
		}
	}
}

export function validatePinConfig(config, schema) {
	validateAgainstSchema(config, schema);
	const expectedUrl = `https://raw.githubusercontent.com/oracle/oci-cli/v${config.ociCli.version}/scripts/install/install.sh`;
	if (config.ociCli.installerUrl !== expectedUrl) {
		throw new Error(`$.ociCli.installerUrl must equal ${expectedUrl}`);
	}
	return config;
}

export async function resolvePinConfig(
	configPath = ".github/ci-pins.jsonc",
	schemaPath = ".github/schemas/ci-pins.schema.jsonc",
) {
	const [configSource, schemaSource] = await Promise.all([
		readFile(configPath, "utf8"),
		readFile(schemaPath, "utf8"),
	]);
	const config = parseJsonc(configSource);
	const schema = parseJsonc(schemaSource);
	validatePinConfig(config, schema);
	return {
		config,
		configSha256: createHash("sha256").update(configSource).digest("hex"),
	};
}

export function pinOutputs(config, configSha256) {
	return {
		runner_ref: config.systemIntegration.runnerRef,
		catalog_ref: config.systemIntegration.catalogRef,
		catalog_schema_version: String(
			config.systemIntegration.catalogSchemaVersion,
		),
		oci_cli_version: config.ociCli.version,
		oci_installer_url: config.ociCli.installerUrl,
		oci_installer_sha256: config.ociCli.installerSha256,
		oci_installer_py_sha256: config.ociCli.installerPySha256,
		config_sha256: configSha256,
	};
}

export async function writeGitHubOutputs(outputPath, outputs) {
	const lines = Object.entries(outputs)
		.map(([key, value]) => `${key}=${value}`)
		.join("\n");
	await appendFile(outputPath, `${lines}\n`, "utf8");
}

async function main() {
	let configPath = ".github/ci-pins.jsonc";
	let schemaPath = ".github/schemas/ci-pins.schema.jsonc";
	let githubOutput;

	for (let index = 2; index < process.argv.length; index += 1) {
		const argument = process.argv[index];
		if (argument === "--config") configPath = process.argv[++index];
		else if (argument === "--schema") schemaPath = process.argv[++index];
		else if (argument === "--github-output")
			githubOutput = process.argv[++index];
		else throw new Error(`Unknown argument: ${argument}`);
	}

	const { config, configSha256 } = await resolvePinConfig(
		configPath,
		schemaPath,
	);
	const outputs = pinOutputs(config, configSha256);
	if (githubOutput) await writeGitHubOutputs(githubOutput, outputs);
	process.stdout.write(`${JSON.stringify(outputs, null, 2)}\n`);
}

if (
	process.argv[1] &&
	import.meta.url === pathToFileURL(process.argv[1]).href
) {
	main().catch((error) => {
		const message = error instanceof Error ? error.message : String(error);
		process.stderr.write(`::error::CI pin resolution failed: ${message}\n`);
		process.exitCode = 1;
	});
}
