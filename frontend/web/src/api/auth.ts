export async function logout(): Promise<void> {
  await fetch("/auth/logout", {
    method: "POST",
    credentials: "same-origin"
  });
}
