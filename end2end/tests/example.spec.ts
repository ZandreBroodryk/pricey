import { test, expect } from "@playwright/test";

test("signed-out visitors are sent to the login page", async ({ page }) => {
  await page.goto("http://localhost:3000/");

  await expect(page).toHaveURL(/\/login$/);
  await expect(page.locator("h1")).toHaveText("Sign in");
  await expect(page.locator('input[name="email"]')).toBeVisible();
  await expect(page.locator('input[name="password"]')).toBeVisible();
});

test("the signup page is reachable from the login page", async ({ page }) => {
  await page.goto("http://localhost:3000/login");
  await page.getByRole("link", { name: "Create one" }).click();

  await expect(page).toHaveURL(/\/signup$/);
  await expect(page.locator("h1")).toHaveText("Create an account");
});
