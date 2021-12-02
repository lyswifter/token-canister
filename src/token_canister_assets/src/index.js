import { token_canister } from "../../declarations/token_canister";

document.getElementById("clickMeBtn").addEventListener("click", async () => {
  const name = document.getElementById("name").value.toString();
  // Interact with token_canister actor, calling the greet method
  const greeting = await token_canister.greet(name);

  document.getElementById("greeting").innerText = greeting;
});
