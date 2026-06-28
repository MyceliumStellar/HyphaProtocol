import { keccak256, toUtf8Bytes } from "ethers";

/**
 * ERC-8004 off-chain Feedback File. The on-chain `give_feedback` call stores `value`/`valueDecimals`/
 * tags and emits a `feedback_hash` committing to this document; the full file (including
 * `proofOfPayment`) lives off-chain and is resolved via the emitted `feedback_uri`.
 */
export interface ProofOfPayment {
  scheme: "x402";
  network: string;
  asset: string;
  amount: string;
  txHash?: string;
}

export interface FeedbackFile {
  agentRegistry: string;
  agentId: number;
  clientAddress: string;
  createdAt: string;
  value: number;
  valueDecimals: number;
  tag1?: string;
  tag2?: string;
  endpoint?: string;
  mcp?: string;
  a2a?: string;
  proofOfPayment?: ProofOfPayment;
}

export interface BuildFeedbackParams {
  identityContractId: string;
  network: string;
  agentId: number;
  clientAddress: string;
  createdAt: string;
  value: number;
  valueDecimals?: number;
  tag1?: string;
  tag2?: string;
  endpoint?: string;
  mcp?: string;
  a2a?: string;
  proofOfPayment?: ProofOfPayment;
}

/** Assemble a spec-compliant feedback file. `agentRegistry` uses the Stellar namespace form. */
export function buildFeedbackFile(p: BuildFeedbackParams): FeedbackFile {
  return {
    agentRegistry: `stellar:${p.network}:${p.identityContractId}`,
    agentId: p.agentId,
    clientAddress: p.clientAddress,
    createdAt: p.createdAt,
    value: p.value,
    valueDecimals: p.valueDecimals ?? 0,
    tag1: p.tag1,
    tag2: p.tag2,
    endpoint: p.endpoint,
    mcp: p.mcp,
    a2a: p.a2a,
    proofOfPayment: p.proofOfPayment,
  };
}

/** Deterministic JSON with sorted keys, so the hash is stable across producers. */
function canonicalJson(value: any): string {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  const keys = Object.keys(value)
    .filter((k) => value[k] !== undefined)
    .sort();
  return `{${keys.map((k) => `${JSON.stringify(k)}:${canonicalJson(value[k])}`).join(",")}}`;
}

/**
 * KECCAK-256 over the canonical feedback file, returned as a 0x-prefixed 32-byte hex string. This is
 * the value passed to `give_feedback`'s `feedback_hash` argument (decode the hex to bytes on-chain).
 */
export function feedbackHash(file: FeedbackFile): string {
  return keccak256(toUtf8Bytes(canonicalJson(file)));
}
