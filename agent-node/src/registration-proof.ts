import { Keypair } from "@stellar/stellar-sdk";

/**
 * Cryptographic ownership proof for an ERC-8004 Agent Registration File on Stellar.
 *
 * ERC-8004 verifies that a registration file genuinely belongs to the on-chain agent. On EVM this is
 * an EIP-712 / ERC-1271 signature; on Stellar we sign the binding claim with the agent's ed25519 key.
 * A verifier then checks (a) the signature is valid for `publicKey`, and (b) `publicKey` matches the
 * agent's on-chain `owner_of(agentId)` or `get_agent_wallet(agentId)` — proving the same key controls
 * both the published domain document and the on-chain identity.
 */
export interface RegistrationProof {
  type: "stellar-ed25519";
  publicKey: string;
  domain: string;
  signature: string; // base64
}

const PREFIX = "HYPHA-AGENT-REGISTRATION-V1";

/** Canonical message bound by the proof: registry namespace + agentId + domain. */
export function registrationMessage(agentRegistry: string, agentId: number, domain: string): string {
  return `${PREFIX}|${agentRegistry}|${agentId}|${domain}`;
}

/** Sign the registration claim with the agent's Stellar secret key. */
export function signRegistration(
  secretKey: string,
  agentRegistry: string,
  agentId: number,
  domain: string
): RegistrationProof {
  const kp = Keypair.fromSecret(secretKey);
  const msg = Buffer.from(registrationMessage(agentRegistry, agentId, domain), "utf-8");
  const signature = kp.sign(msg).toString("base64");
  return { type: "stellar-ed25519", publicKey: kp.publicKey(), domain, signature };
}

/**
 * Verify a registration proof's signature (step a). Returns false on any malformed input rather than
 * throwing. Callers should additionally confirm `proof.publicKey` equals the agent's on-chain owner
 * or agent wallet (step b) before trusting the domain binding.
 */
export function verifyRegistrationSignature(
  proof: RegistrationProof,
  agentRegistry: string,
  agentId: number
): boolean {
  try {
    if (proof.type !== "stellar-ed25519") return false;
    const kp = Keypair.fromPublicKey(proof.publicKey);
    const msg = Buffer.from(registrationMessage(agentRegistry, agentId, proof.domain), "utf-8");
    return kp.verify(msg, Buffer.from(proof.signature, "base64"));
  } catch {
    return false;
  }
}
