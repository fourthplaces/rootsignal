import { useState } from "react";
import { useNavigate } from "react-router";
import { useMutation } from "@apollo/client";
import { SEND_OTP, VERIFY_OTP } from "@/graphql/mutations";

export function LoginPage() {
  const navigate = useNavigate();
  const [step, setStep] = useState<"phone" | "code">("phone");
  const [phone, setPhone] = useState("");
  const [code, setCode] = useState("");
  const [error, setError] = useState("");

  const [sendOtp, { loading: sending }] = useMutation(SEND_OTP);
  const [verifyOtp, { loading: verifying }] = useMutation(VERIFY_OTP);

  const handleSendOtp = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    try {
      const { data } = await sendOtp({ variables: { phone } });
      if (data?.sendOtp?.success) {
        setStep("code");
      } else {
        setError("Could not send code. Check the phone number.");
      }
    } catch {
      setError("Failed to send OTP.");
    }
  };

  const handleVerifyOtp = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    try {
      const { data } = await verifyOtp({ variables: { phone, code } });
      if (data?.verifyOtp?.success) {
        navigate("/", { replace: true });
      } else {
        setError("Invalid code. Try again.");
      }
    } catch {
      setError("Verification failed.");
    }
  };

  return (
    <div className="flex h-screen items-center justify-center">
      <div className="w-full max-w-sm space-y-6">
        <div className="text-center">
          <h1 className="text-2xl font-semibold">Root Signal</h1>
          <p className="text-sm text-muted-foreground">Admin Login</p>
        </div>

        {step === "phone" ? (
          <form onSubmit={handleSendOtp} className="space-y-4">
            <div>
              <label className="block text-sm mb-1.5">Phone Number</label>
              <input
                type="tel"
                value={phone}
                onChange={(e) => setPhone(e.target.value)}
                placeholder="+1234567890"
                className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm focus:outline-none focus:ring-2 focus:ring-ring"
                required
              />
            </div>
            {error && <p className="text-sm text-red-400">{error}</p>}
            <button
              type="submit"
              disabled={sending}
              className="w-full px-4 py-2 rounded-md bg-primary text-primary-foreground text-sm font-medium hover:bg-primary/90 disabled:opacity-50"
            >
              {sending ? "Sending..." : "Send Code"}
            </button>
          </form>
        ) : (
          <form onSubmit={handleVerifyOtp} className="space-y-4">
            <div>
              <label className="block text-sm mb-1.5">Verification Code</label>
              <input
                type="text"
                value={code}
                onChange={(e) => setCode(e.target.value)}
                placeholder="123456"
                maxLength={6}
                className="w-full px-3 py-2 rounded-md border border-input bg-background text-sm focus:outline-none focus:ring-2 focus:ring-ring tracking-widest text-center text-lg"
                required
              />
            </div>
            {error && <p className="text-sm text-red-400">{error}</p>}
            <button
              type="submit"
              disabled={verifying}
              className="w-full px-4 py-2 rounded-md bg-primary text-primary-foreground text-sm font-medium hover:bg-primary/90 disabled:opacity-50"
            >
              {verifying ? "Verifying..." : "Verify"}
            </button>
            <button
              type="button"
              onClick={() => {
                setStep("phone");
                setCode("");
                setError("");
              }}
              className="w-full text-sm text-muted-foreground hover:text-foreground"
            >
              Use different number
            </button>
          </form>
        )}
      </div>
    </div>
  );
}
