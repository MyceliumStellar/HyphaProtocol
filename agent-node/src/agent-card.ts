import { Router } from "express";
import { Wallet } from "ethers";
import * as dotenv from "dotenv";
import * as path from "path";
import { fileURLToPath } from "url";
import { signRegistration, RegistrationProof } from "./registration-proof.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Load environment variables
dotenv.config({ path: path.join(__dirname, "../.env") });

export interface CommunicationEndpoint {
  type: "http" | "xmtp";
  url_or_address: string;
}

/**
 * ERC-8004 global agent identifier: `{namespace}:{network}:{identityRegistry}` + on-chain agentId.
 * Lets external indexers resolve this agent's on-chain identity across registries.
 */
export interface OnChainRegistration {
  agentRegistry: string;
  agentId: number;
  /** ed25519 ownership proof binding this domain to the on-chain agentId (see registration-proof.ts). */
  proof?: RegistrationProof;
}

export interface AgentCard {
  id: string;
  name: string;
  description: string;
  version: string;
  communication_endpoints: CommunicationEndpoint[];
  authentication: { type: "x402"; asset: "USDC"; network: string };
  capabilities: string[];
  // ERC-8004 trust layer pointers.
  registrations: OnChainRegistration[];
  supportedTrust: string[];
}

/**
 * ERC-8004 Agent Registration File — the document an `agentURI` resolves to. Mirrors the spec's
 * mandatory `type`/`name`/`description`/`image`, the `services` array, and the trust metadata. The
 * same file is published at `/.well-known/agent-registration.json` for endpoint domain verification.
 */
export interface AgentRegistrationFile {
  type: string;
  name: string;
  description: string;
  image: string;
  services: { type: string; url: string }[];
  x402Support: boolean;
  active: boolean;
  registrations: OnChainRegistration[];
  supportedTrust: string[];
}

const stellarNetwork = process.env.STELLAR_NETWORK || "testnet";
const agentPublicUrl = process.env.AGENT_PUBLIC_URL || "http://localhost:3000";
const federationName = process.env.FEDERATION_NAME || "oracle*hypha.network";
const identityContractId =
  process.env.IDENTITY_CONTRACT_ID || process.env.REGISTRY_CONTRACT_ID || "";
const agentId = Number(process.env.AGENT_ID || "0");
const agentSecret = process.env.AGENT_SECRET_KEY || "";

function domainHost(): string {
  try {
    return new URL(agentPublicUrl).host;
  } catch {
    return agentPublicUrl;
  }
}

// Derive EVM Wallet Address for XMTP endpoint configuration.
let evmWalletAddress = "0x0000000000000000000000000000000000000000";
const evmPrivateKey = process.env.EVM_PRIVATE_KEY;
if (evmPrivateKey) {
  try {
    evmWalletAddress = new Wallet(evmPrivateKey).address;
  } catch (error) {
    console.error("[AgentCard] Failed to parse EVM_PRIVATE_KEY to derive address:", error);
  }
}

/**
 * Build the on-chain registration pointer from the Identity Registry namespace + this node's agentId,
 * attaching a cryptographic ownership proof when a real signing key is configured.
 */
function buildRegistrations(): OnChainRegistration[] {
  if (!identityContractId || !agentId) return [];
  const agentRegistry = `stellar:${stellarNetwork}:${identityContractId}`;
  const reg: OnChainRegistration = { agentRegistry, agentId };
  if (agentSecret && !agentSecret.startsWith("SDUMMY")) {
    try {
      reg.proof = signRegistration(agentSecret, agentRegistry, agentId, domainHost());
    } catch (error) {
      console.error("[AgentCard] Failed to sign registration proof:", error);
    }
  }
  return [reg];
}

function buildAgentCard(): AgentCard {
  return {
    id: federationName,
    name: "Hypha Oracle Agent",
    description:
      "Decentralized P2P agent providing Stellar USDC balance query capabilities and registry indexes.",
    version: "1.0.0",
    communication_endpoints: [
      { type: "http", url_or_address: `${agentPublicUrl}/agent/execute` },
      { type: "xmtp", url_or_address: evmWalletAddress },
    ],
    authentication: { type: "x402", asset: "USDC", network: `stellar-${stellarNetwork}` },
    capabilities: ["get_balance", "query_registry"],
    registrations: buildRegistrations(),
    supportedTrust: ["feedback", "validation"],
  };
}

function buildRegistrationFile(): AgentRegistrationFile {
  return {
    type: "https://eips.ethereum.org/EIPS/eip-8004#agent",
    name: "Hypha Oracle Agent",
    description:
      "Decentralized P2P agent providing Stellar USDC balance query capabilities and registry indexes.",
    image: `${agentPublicUrl}/static/agent.png`,
    services: [
      { type: "A2A", url: `${agentPublicUrl}/.well-known/agent-card.json` },
      { type: "MCP", url: `${agentPublicUrl}/agent/execute` },
      { type: "web", url: agentPublicUrl },
    ],
    x402Support: true,
    active: true,
    registrations: buildRegistrations(),
    supportedTrust: ["feedback", "validation"],
  };
}

export const agentCardRouter = Router();

/**
 * GET /.well-known/agent-card.json
 * A2A AgentCard (current spec filename) extended with ERC-8004 `registrations` + `supportedTrust`.
 */
agentCardRouter.get("/.well-known/agent-card.json", (_req, res) => {
  res.setHeader("Access-Control-Allow-Origin", "*");
  res.setHeader("Content-Type", "application/json");
  res.status(200).json(buildAgentCard());
});

/**
 * GET /.well-known/agent.json
 * Backwards-compatible alias — A2A renamed the file to agent-card.json.
 */
agentCardRouter.get("/.well-known/agent.json", (_req, res) => {
  res.redirect(301, "/.well-known/agent-card.json");
});

/**
 * GET /.well-known/agent-registration.json
 * ERC-8004 Agent Registration File, also used for endpoint domain verification (its `registrations`
 * entry must match the on-chain agentURI).
 */
agentCardRouter.get("/.well-known/agent-registration.json", (_req, res) => {
  res.setHeader("Access-Control-Allow-Origin", "*");
  res.setHeader("Content-Type", "application/json");
  res.status(200).json(buildRegistrationFile());
});
