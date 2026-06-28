import { Router } from "express";
import { rpc, Keypair, Account, TransactionBuilder, Operation, nativeToScVal, scValToNative } from "@stellar/stellar-sdk";
import * as dotenv from "dotenv";
import * as path from "path";
import * as fs from "fs";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Load environment variables
dotenv.config({ path: path.join(__dirname, "../.env") });

export const federationRouter = Router();

const agentsConfigPath = path.join(__dirname, "../config/agents.json");
const agentPublicUrl = process.env.AGENT_PUBLIC_URL || "http://localhost:3000";

interface LocalAgent {
  stellar_address: string;
  account_id: string;
  mcp_endpoint: string;
  agent_id: number;
}

/**
 * Dynamically queries the deployed Soroban Identity Registry on testnet to verify that the given
 * numeric agentId is actually registered on-chain (ERC-8004 `agent_exists`).
 */
async function verifyAgentOnChain(agentId: number): Promise<boolean> {
  const contractId = process.env.IDENTITY_CONTRACT_ID || process.env.REGISTRY_CONTRACT_ID;
  if (!contractId) {
    console.log("[Federation] No IDENTITY_CONTRACT_ID configured. Skipping on-chain registration check.");
    return true; // Fallback to config-only verification if contract ID is not configured
  }
  if (!Number.isInteger(agentId) || agentId <= 0) {
    console.log(`[Federation] Local record has no valid agent_id (${agentId}).`);
    return false;
  }

  try {
    const server = new rpc.Server("https://soroban-testnet.stellar.org");

    // Construct a dummy transaction for simulation using a dynamic random Keypair.
    // Soroban simulations do not verify signatures or require funded accounts, so this is perfectly safe.
    const simulationAccount = new Account(Keypair.random().publicKey(), "0");

    const tx = new TransactionBuilder(simulationAccount, {
      fee: "100",
      networkPassphrase: "Test SDF Network ; September 2015",
    })
      .addOperation(
        Operation.invokeContractFunction({
          contract: contractId,
          function: "agent_exists",
          args: [nativeToScVal(BigInt(agentId), { type: "u64" })],
        })
      )
      .setTimeout(30)
      .build();

    console.log(`[Federation] Simulating agent_exists(${agentId}) on Identity Registry ${contractId}...`);
    const simRes = await server.simulateTransaction(tx);

    if (rpc.Api.isSimulationSuccess(simRes)) {
      const retval = simRes.result?.retval;
      if (!retval) {
        return false;
      }
      const val = scValToNative(retval);
      console.log(`[Federation] On-chain verification result:`, val);
      return val === true;
    } else {
      console.error("[Federation] On-chain simulation failed. Response:", simRes);
      return false;
    }
  } catch (error) {
    console.error("[Federation] Error querying Identity Registry contract:", error);
    return false;
  }
}

/**
 * GET /.well-known/stellar.toml
 * Exposes the Federation Server URL to the Stellar network client resolvers.
 */
federationRouter.get("/.well-known/stellar.toml", (req, res) => {
  res.setHeader("Content-Type", "text/plain");
  res.setHeader("Access-Control-Allow-Origin", "*");
  res.send(`FEDERATION_SERVER="${agentPublicUrl}/federation"\n`);
});

/**
 * GET /federation
 * Handles SEP-0002 lookup requests by query q.
 */
federationRouter.get("/federation", async (req, res) => {
  const { q, type } = req.query;

  // Set CORS header strictly required by the SEP-0002 spec
  res.setHeader("Access-Control-Allow-Origin", "*");

  if (!q || type !== "name") {
    return res.status(400).json({ error: "Unsupported query. Only lookup 'type=name' is supported." });
  }

  const queryName = typeof q === "string" ? q.trim().toLowerCase() : "";
  const parts = queryName.split("*");
  if (parts.length !== 2) {
    return res.status(400).json({ error: "Invalid federation address format. Must be user*domain." });
  }

  const agentName = parts[0];

  try {
    // Read the local agents database mapping
    if (!fs.existsSync(agentsConfigPath)) {
      console.error(`[Federation] Configuration file not found at ${agentsConfigPath}`);
      return res.status(500).json({ error: "Server configuration error" });
    }

    const fileContent = fs.readFileSync(agentsConfigPath, "utf-8");
    const agentsDb: Record<string, LocalAgent> = JSON.parse(fileContent);

    const agentRecord = agentsDb[agentName];
    if (!agentRecord) {
      console.log(`[Federation] Name lookup failed for: ${agentName}`);
      return res.status(404).json({ error: "Agent not found in registry" });
    }

    // Override the wallet address with our active testing recipient address if configured
    const resolvedAccountId = (agentName === "oracle" && process.env.PAYMENT_RECIPIENT_ADDRESS)
      ? process.env.PAYMENT_RECIPIENT_ADDRESS
      : agentRecord.account_id;

    // Perform the dynamic on-chain validation check to make sure the agent's numeric agentId is
    // registered in the Identity Registry on testnet.
    const isRegisteredOnChain = await verifyAgentOnChain(agentRecord.agent_id);
    if (!isRegisteredOnChain) {
      console.log(`[Federation] Agent #${agentRecord.agent_id} is configured locally but has no active on-chain registration.`);
      return res.status(404).json({ error: "Agent not found in registry" });
    }

    const identityContractId = process.env.IDENTITY_CONTRACT_ID || process.env.REGISTRY_CONTRACT_ID || "";
    const network = process.env.STELLAR_NETWORK || "testnet";

    console.log(`[Federation] SEP-0002 query resolved successfully for: ${queryName}`);
    return res.status(200).json({
      stellar_address: agentRecord.stellar_address,
      account_id: resolvedAccountId,
      mcp_endpoint: `${agentPublicUrl}/agent/execute`,
      agent_id: agentRecord.agent_id,
      agent_registry: identityContractId ? `stellar:${network}:${identityContractId}` : undefined,
    });
  } catch (error) {
    console.error("[Federation] Exception resolving federation query:", error);
    return res.status(500).json({ error: "Internal server error resolving query" });
  }
});
