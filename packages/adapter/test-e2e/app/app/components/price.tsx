export function Price({ amount, currency = "USD" }: { amount: number; currency?: string }) {
  return (
    <p className="price">
      {amount.toFixed(2)} <span>{currency}</span>
    </p>
  );
}
