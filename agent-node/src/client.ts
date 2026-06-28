import { Keypair } from "@stellar/stellar-sdk";
import { wrapFetchWithPayment } from "stellar-x402/client-http";
import * as dotenv from "dotenv";
import * as path from "path";

// Load environment variables
dotenv.config({ path: path.join(__dirname, "../.env") });

const agentSecret = process.env.AGENT_SECRET_KEY;
if (!agentSecret || agentSecret.startsWith("SDUMMY")) {
  console.error("Error: AGENT_SECRET_KEY is not configured with a valid Stellar secret key.");
  console.log("Please update agent-node/.env with a valid Stellar Testnet secret key.");
  process.exit(1);
}

// Generate the keypair from secret key
let keypair: Keypair;
try {
  keypair = Keypair.fromSecret(agentSecret);
} catch (err) {
  console.error("Invalid AGENT_SECRET_KEY format:", err);
  process.exit(1);
}

console.log("--------------------------------------------------");
console.log("Starting Stellar x402 Programmatic Client:");
console.log(`- Agent Public Key: ${keypair.publicKey()}`);
console.log("--------------------------------------------------");

// Wrap the global fetch function.
// wrapFetchWithPayment automatically intercept 402 challenges, signs the required Soroban authorization
// payload using the provided keypair, submits the payment to the network via the RPC, and retries the request.
const fetchWithPayment = wrapFetchWithPayment(
  fetch,
  keypair,
  10000000n, // Max allowed payment (1.0 USDC = 10,000,000 stroops)
  undefined,
  {
    stellarConfig: {
      rpcUrl: "https://soroban-testnet.stellar.org",
    },
  }
);

async function executeAgentAction() {
  const targetUrl = "http://localhost:3000/agent/execute";
  console.log(`[Client] Sending POST request to protected route: ${targetUrl}...`);

  try {
    const response = await fetchWithPayment(targetUrl, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        action: "defi_swap",
        params: {
          fromAsset: "USDC",
          toAsset: "XLM",
          amount: "10",
        },
      }),
    });

    console.log(`[Client] Received response. Status code: ${response.status}`);
    
    if (response.ok) {
      const data = await response.json();
      console.log("[Client] Request Succeeded!");
      console.log(JSON.stringify(data, null, 2));
    } else {
      const errorText = await response.text();
      console.error(`[Client] Request failed with status ${response.status}:`, errorText);
    }
  } catch (error) {
    console.error("[Client] Unexpected error during request execution:", error);
  }
}

// Execute the call
executeAgentAction();
