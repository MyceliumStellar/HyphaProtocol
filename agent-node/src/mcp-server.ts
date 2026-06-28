import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { ListToolsRequestSchema, CallToolRequestSchema } from "@modelcontextprotocol/sdk/types.js";
import { rpc, Keypair, Account, TransactionBuilder, Operation, nativeToScVal, scValToNative } from "@stellar/stellar-sdk";
import { StellarAgentKit } from "stellar-agent-kit";
import * as dotenv from "dotenv";
import * as path from "path";

// Load environment variables
dotenv.config({ path: path.join(__dirname, "../.env") });

const agentSecret = process.env.AGENT_SECRET_KEY;
if (!agentSecret || agentSecret.startsWith("SDUMMY")) {
  throw new Error("Missing or invalid AGENT_SECRET_KEY in environment variables");
}

// Initialize Stellar Agent Kit
const kit = new StellarAgentKit(agentSecret);

// Overwrite the kit config to target Testnet for querying Horizon/Soroban testnet endpoints
(kit as any).config = {
  horizonUrl: "https://horizon-testnet.stellar.org",
  sorobanRpcUrl: "https://soroban-testnet.stellar.org",
  friendbotUrl: "https://friendbot.stellar.org",
};

let kitInitialized = false;

// Helper to ensure the kit is initialized before use
async function getKit() {
  if (!kitInitialized) {
    await kit.initialize();
    kitInitialized = true;
  }
  return kit;
}

const SOROBAN_RPC_URL = "https://soroban-testnet.stellar.org";
const NETWORK_PASSPHRASE = "Test SDF Network ; September 2015";

/**
 * Read a Soroban contract view function via transaction simulation (no signatures, no funded account
 * required) and return the decoded native JS value. Throws on simulation failure.
 */
async function simulateRead(contractId: string, fn: string, args: any[]): Promise<any> {
  const server = new rpc.Server(SOROBAN_RPC_URL);
  const source = new Account(Keypair.random().publicKey(), "0");
  const tx = new TransactionBuilder(source, {
    fee: "100",
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(Operation.invokeContractFunction({ contract: contractId, function: fn, args }))
    .setTimeout(30)
    .build();

  const sim = await server.simulateTransaction(tx);
  if (!rpc.Api.isSimulationSuccess(sim)) {
    throw new Error(`simulation of ${fn} failed: ${JSON.stringify(sim)}`);
  }
  const retval = sim.result?.retval;
  return retval ? scValToNative(retval) : null;
}

// Initialize the MCP Server
export const mcpServer = new Server(
  {
    name: "stellar-a2a-mcp-server",
    version: "1.0.0",
  },
  {
    capabilities: {
      tools: {},
    },
  }
);

// Register Available Tools
mcpServer.setRequestHandler(ListToolsRequestSchema, async () => {
  return {
    tools: [
      {
        name: "get_balance",
        description: "Query the USDC balance of any Stellar address on Testnet using StellarAgentKit.",
        inputSchema: {
          type: "object",
          properties: {
            address: {
              type: "string",
              description: "Stellar account public key (starting with G, 56 characters).",
            },
          },
          required: ["address"],
        },
      },
      {
        name: "query_registry",
        description: "Resolve an agent's on-chain identity (owner, agentURI, operational wallet) from the ERC-8004 Identity Registry by its numeric agentId.",
        inputSchema: {
          type: "object",
          properties: {
            agent_id: {
              type: "integer",
              description: "Numeric agentId assigned by the Identity Registry (e.g. 1).",
            },
          },
          required: ["agent_id"],
        },
      },
    ],
  };
});

// Handle Tool Executions
mcpServer.setRequestHandler(CallToolRequestSchema, async (request) => {
  const { name, arguments: args } = request.params;

  try {
    if (name === "get_balance") {
      const address = String(args?.address);
      if (!address || !address.startsWith("G") || address.length !== 56) {
        return {
          isError: true,
          content: [{ type: "text", text: "Invalid Stellar public key format." }],
        };
      }

      console.log(`[MCP Server] Tool get_balance called for address: ${address}`);
      const agentKit = await getKit();
      const balances = await agentKit.getBalances(address);
      const usdcBalance = balances.find((b) => b.assetCode === "USDC")?.balance || "0";

      return {
        content: [
          {
            type: "text",
            text: JSON.stringify({
              address,
              usdc_balance: usdcBalance,
              balances,
            }),
          },
        ],
      };
    } else if (name === "query_registry") {
      const agentId = Number(args?.agent_id);
      if (!Number.isInteger(agentId) || agentId <= 0) {
        return {
          isError: true,
          content: [{ type: "text", text: "Invalid agent_id: must be a positive integer." }],
        };
      }

      console.log(`[MCP Server] Tool query_registry called for agentId: ${agentId}`);
      const contractId = process.env.IDENTITY_CONTRACT_ID || process.env.REGISTRY_CONTRACT_ID;
      if (!contractId) {
        return {
          isError: true,
          content: [{ type: "text", text: "IDENTITY_CONTRACT_ID is not configured on the server." }],
        };
      }

      try {
        const idArg = nativeToScVal(BigInt(agentId), { type: "u64" });

        const exists = await simulateRead(contractId, "agent_exists", [idArg]);
        if (!exists) {
          return {
            content: [{ type: "text", text: JSON.stringify({ agent_id: agentId, registered: false, message: "Agent does not exist in the Identity Registry." }) }],
          };
        }

        // Resolve identity fields from the ERC-8004 Identity Registry.
        const owner = await simulateRead(contractId, "owner_of", [idArg]);
        const agentUri = await simulateRead(contractId, "agent_uri", [idArg]);
        const agentWallet = await simulateRead(contractId, "get_agent_wallet", [idArg]);

        return {
          content: [
            {
              type: "text",
              text: JSON.stringify({
                agent_id: agentId,
                owner,
                agent_uri: agentUri,
                agent_wallet: agentWallet,
                registered: true,
                source: `On-chain Identity Registry (${contractId})`,
              }),
            },
          ],
        };
      } catch (err: any) {
        return {
          isError: true,
          content: [{ type: "text", text: `Failed to query Identity Registry on-chain: ${err.message || err}` }],
        };
      }
    } else {
      throw new Error(`Tool ${name} not found.`);
    }
  } catch (error: any) {
    return {
      isError: true,
      content: [{ type: "text", text: `Error: ${error.message || error}` }],
    };
  }
});
