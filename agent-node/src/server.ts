import express from "express";
import { paymentMiddleware } from "stellar-x402/server";
import { SSEServerTransport } from "@modelcontextprotocol/sdk/server/sse.js";
import { mcpServer } from "./mcp-server.js";
import { federationRouter } from "./federation.js";
import { agentCardRouter } from "./agent-card.js";
import * as dotenv from "dotenv";
import * as path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Load environment variables
dotenv.config({ path: path.join(__dirname, "../.env") });

const app = express();
const PORT = process.env.PORT || 3000;

app.use(express.json());

// Mount the SEP-0002 Federation Protocol Router
app.use("/", federationRouter);

// Mount the A2A Standardized Agent Card Router
app.use("/", agentCardRouter);

const paymentRecipient = process.env.PAYMENT_RECIPIENT_ADDRESS;
const facilitatorUrl = process.env.FACILITATOR_URL;

if (!paymentRecipient) {
  throw new Error("Missing PAYMENT_RECIPIENT_ADDRESS in environment variables");
}
if (!facilitatorUrl) {
  throw new Error("Missing FACILITATOR_URL in environment variables");
}

console.log("--------------------------------------------------");
console.log("Initializing Stellar x402 + MCP Agent Server:");
console.log(`- Recipient Address: ${paymentRecipient}`);
console.log(`- Facilitator URL: ${facilitatorUrl}`);
console.log("--------------------------------------------------");

// Create the payment middleware handler for the MCP endpoint
const protectMcp = paymentMiddleware(
  paymentRecipient,
  {
    "/agent/execute": {
      price: "$0.01",
      network: "stellar-testnet",
      config: {
        description: "Stellar A2A Agent Tool Connection Charge",
        discoverable: true,
      },
    },
  },
  {
    url: facilitatorUrl as `${string}://${string}`,
  }
);

let transport: SSEServerTransport | undefined;

// GET /agent/execute: Establish the SSE connection channel (protected by paymentMiddleware)
app.get("/agent/execute", protectMcp, async (req, res) => {
  console.log("[SSE] Payment verified. Establishing MCP Server connection...");
  
  // SSEServerTransport communicates to the client where to post subsequent messages
  transport = new SSEServerTransport("/agent/execute/messages", res);
  await mcpServer.connect(transport);
  
  console.log("[SSE] MCP Server connected to transport channel.");
});

// POST /agent/execute/messages: Receive JSON-RPC messages from the client (connection is already paid)
app.post("/agent/execute/messages", async (req, res) => {
  console.log("[SSE] Received message packet from client");
  if (transport) {
    await transport.handlePostMessage(req, res);
  } else {
    res.status(400).send("SSE connection not established. Connect via GET /agent/execute first.");
  }
});

// Unprotected health check route
app.get("/health", (req, res) => {
  res.status(200).json({
    status: "OK",
    timestamp: new Date().toISOString(),
    message: "Stellar A2A Payment Middleware & MCP Server are online.",
  });
});

app.listen(PORT, () => {
  console.log(`[Server] SSE MCP Server listening at http://localhost:${PORT}`);
});
