import { Client, Signer, IdentifierKind } from "@xmtp/node-sdk";
import { Wallet, getBytes } from "ethers";
import { randomBytes } from "crypto";
import * as dotenv from "dotenv";
import * as path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Load environment variables
dotenv.config({ path: path.join(__dirname, "../.env") });

const privateKey = process.env.EVM_PRIVATE_KEY;
if (!privateKey) {
  console.error("Error: EVM_PRIVATE_KEY is not defined in environment variables.");
  process.exit(1);
}

const agentPublicUrl = process.env.AGENT_PUBLIC_URL || "http://localhost:3000";

async function startXmtpListener() {
  let wallet: Wallet;
  try {
    wallet = new Wallet(privateKey!);
  } catch (err) {
    console.error("Invalid EVM_PRIVATE_KEY format. Must be a 32-byte hex string starting with 0x.", err);
    process.exit(1);
  }

  console.log("--------------------------------------------------");
  console.log("Starting XMTP P2P Agent Listener:");
  console.log(`- Ethereum Wallet Address: ${wallet.address}`);
  console.log("--------------------------------------------------");

  // Define XMTP Signer wrapper for the ethers wallet
  const signer: Signer = {
    type: "EOA",
    getIdentifier: () => ({
      identifier: wallet.address,
      identifierKind: IdentifierKind.Ethereum,
    }),
    signMessage: async (message: string): Promise<Uint8Array> => {
      const signature = await wallet.signMessage(message);
      return getBytes(signature);
    },
  };

  // Generate random DB encryption key for SQLite instance
  const dbEncryptionKey = new Uint8Array(randomBytes(32));

  try {
    // Create XMTP Client using dev environment (testnet sandbox)
    const client = await Client.create(signer, {
      env: "dev",
      dbEncryptionKey,
    } as any);

    console.log(`[XMTP] Client created successfully! Inbox ID: ${client.inboxId}`);
    console.log("[XMTP] Listening for incoming P2P messages...");

    // Stream all conversations/messages in real-time
    const stream = await client.conversations.streamAllMessages();

    for await (const message of stream) {
      // Ignore messages sent by our own agent inbox to avoid infinite loops
      if (message.senderInboxId === client.inboxId) {
        continue;
      }

      console.log(`[XMTP] Message received from inbox ${message.senderInboxId}:`);
      console.log(`- Content: "${message.content}"`);

      try {
        const conversation = await client.conversations.getConversationById(message.conversationId);
        if (conversation) {
          const replyText = 
            `Hello! I am a Hypha Protocol Agent. My MCP execution endpoint is ${agentPublicUrl}/agent/execute. ` +
            "Please send an HTTP GET request there to initiate the x402 payment challenge and connect.";

          console.log(`[XMTP] Replying to conversation ${conversation.id}...`);
          await conversation.sendText(replyText);
          console.log("[XMTP] Reply sent successfully.");
        } else {
          console.error(`[XMTP] Could not find conversation for ID: ${message.conversationId}`);
        }
      } catch (err) {
        console.error("[XMTP] Error responding to message:", err);
      }
    }
  } catch (error) {
    console.error("[XMTP] Fatal error starting listener:", error);
    process.exit(1);
  }
}

startXmtpListener();
