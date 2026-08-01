#!/usr/bin/env node

import assert from "node:assert/strict";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
	parseJsonc,
	pinOutputs,
	resolvePinConfig,
	validatePinConfig,
	writeGitHubOutputs,
} from "./resolve-ci-pins.mjs";

const configPath = ".github/ci-pins.jsonc";
const schemaPath = ".github/schemas/ci-pins.schema.jsonc";

async function fixtures() {
	const [configSource, schemaSource] = await Promise.all([
		readFile(configPath, "utf8"),
		readFile(schemaPath, "utf8"),
	]);
	return {
		config: parseJsonc(configSource),
		schema: parseJsonc(schemaSource),
	};
}

test("parses comments and trailing commas", () => {
	assert.deepEqual(
		parseJsonc('{\n// line\n"a": 1, "b": [true,], /* block */\n}'),
		{ a: 1, b: [true] },
	);
});

test("rejects duplicate keys", () => {
	assert.throws(() => parseJsonc('{"a": 1, "a": 2}'), /Duplicate key 'a'/);
});

test("resolves the committed registry", async () => {
	const { config, configSha256 } = await resolvePinConfig(
		configPath,
		schemaPath,
	);
	assert.match(config.systemIntegration.runnerRef, /^[0-9a-f]{40}$/);
	assert.match(config.systemIntegration.catalogRef, /^[0-9a-f]{40}$/);
	assert.match(configSha256, /^[0-9a-f]{64}$/);
});

test("rejects missing and unknown keys", async () => {
	const { config, schema } = await fixtures();
	const missing = structuredClone(config);
	delete missing.systemIntegration.catalogRef;
	assert.throws(
		() => validatePinConfig(missing, schema),
		/catalogRef is required/,
	);

	const unknown = structuredClone(config);
	unknown.systemIntegration.repository = "another/repository";
	assert.throws(
		() => validatePinConfig(unknown, schema),
		/repository is not allowed/,
	);
});

test("rejects mutable refs and malformed checksums", async () => {
	const { config, schema } = await fixtures();
	const mutable = structuredClone(config);
	mutable.systemIntegration.runnerRef = "main";
	assert.throws(
		() => validatePinConfig(mutable, schema),
		/runnerRef does not match/,
	);

	const checksum = structuredClone(config);
	checksum.ociCli.installerSha256 = "abc";
	assert.throws(
		() => validatePinConfig(checksum, schema),
		/installerSha256 does not match/,
	);
});

test("rejects OCI URL and version mismatch", async () => {
	const { config, schema } = await fixtures();
	const mismatch = structuredClone(config);
	mismatch.ociCli.version = "3.90.0";
	assert.throws(
		() => validatePinConfig(mismatch, schema),
		/installerUrl must equal/,
	);
});

test("writes stable GitHub outputs", async () => {
	const { config, configSha256 } = await resolvePinConfig(
		configPath,
		schemaPath,
	);
	const directory = await mkdtemp(join(tmpdir(), "ci-pins-"));
	const outputPath = join(directory, "github-output");
	await writeFile(outputPath, "", "utf8");
	await writeGitHubOutputs(outputPath, pinOutputs(config, configSha256));
	const output = await readFile(outputPath, "utf8");
	assert.match(
		output,
		new RegExp(`runner_ref=${config.systemIntegration.runnerRef}`),
	);
	assert.match(
		output,
		new RegExp(`catalog_ref=${config.systemIntegration.catalogRef}`),
	);
	assert.match(output, /config_sha256=[0-9a-f]{64}/);
});
