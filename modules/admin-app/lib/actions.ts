"use server";

import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import { client } from "./client";

export async function sendVerificationCode(phone: string) {
  const result = await client.mutate<{ sendVerificationCode: boolean }>(
    `mutation SendCode($phone: String!) { sendVerificationCode(phone: $phone) }`,
    { phone },
  );
  return result.sendVerificationCode;
}

export async function verifyCode(phone: string, code: string) {
  const result = await client.mutate<{ verifyCode: string }>(
    `mutation VerifyCode($phone: String!, $code: String!) { verifyCode(phone: $phone, code: $code) }`,
    { phone, code },
  );

  const token = result.verifyCode;
  const cookieStore = await cookies();
  cookieStore.set("auth_token", token, {
    httpOnly: true,
    secure: process.env.SECURE_COOKIES === "true",
    sameSite: "lax",
    maxAge: 60 * 60 * 24, // 24 hours
    path: "/",
  });

  redirect("/");
}

export async function logout() {
  const cookieStore = await cookies();
  cookieStore.delete("auth_token");
  redirect("/login");
}
