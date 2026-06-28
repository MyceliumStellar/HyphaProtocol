import express from "express";
import * as dotenv from "dotenv";

// Load environment variables
dotenv.config();

const app = express();
const PORT = process.env.PORT || 3000;

app.use(express.json());

// Basic health check route
app.get("/health", (req, res) => {
  res.json({
    status: "OK",
    timestamp: new Date().toISOString(),
    message: "Stellar A2A Protocol Agent Node running.",
  });
});

app.listen(PORT, () => {
  console.log(`Agent Node is listening on port ${PORT}`);
});
