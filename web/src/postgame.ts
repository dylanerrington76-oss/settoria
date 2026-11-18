// Postgame%: how likely each player was to win, judged only from how much dice income they collected.
//
// 1. x1 is a player's dice income minus the leader's, so the leader is always 0 and everyone else is
//    zero or below (income 10 and 8 give x1 = 0 and -2).
// 2. Each player's probability comes from the logistic regression  P = 1 / (1 + e^-(0.0889 + 0.1795 * x1)).
// 3. Those probabilities do not add up to 1 on their own, so they are normalised: with S the sum of all of
//    them, a player's Postgame% is p / S, which makes the column total exactly 100%.

export const POSTGAME_INTERCEPT = 0.0889;
export const POSTGAME_SLOPE = 0.1795;

/** The logistic model: probability for a player who is `x1` dice income away from the leader (0 or negative). */
export function postgameModel(x1: number): number {
  return 1 / (1 + Math.exp(-(POSTGAME_INTERCEPT + POSTGAME_SLOPE * x1)));
}

/**
 * Postgame% for every player, as fractions that add up to 1, in the same order as `diceIncome`.
 * Returns an empty array when there is nobody to rate.
 */
export function postgamePercent(diceIncome: number[]): number[] {
  if (diceIncome.length === 0) return [];
  const leader = Math.max(...diceIncome);
  const probs = diceIncome.map((income) => postgameModel(income - leader));
  const sum = probs.reduce((a, b) => a + b, 0);
  return probs.map((p) => p / sum);
}

/** "54.7%": a fraction shown as a percentage with one decimal. */
export function formatPostgame(fraction: number): string {
  return `${(fraction * 100).toFixed(1)}%`;
}
