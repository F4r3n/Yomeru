# Grammar dictionary: open questions

Entries in `data/grammar.toml` that work but carry a doubt worth deciding
later. Add to this list batch by batch; remove an item once it's settled.

## Hover noise (popup matching inside unrelated words)

- **`aida-ni` (間に)**: hover is on, so hovering the 間 of 時間に or
  間に合う shows this entry. Options: `hover = false`, or rely on JMdict
  winning the longer match. (N4 batch 5)
- **`te-ita` (ていた)**: lists the casual form てた, which matches inside
  捨てた, 建てた, 育てた. `te-iru` already has the same issue with てる
  (捨てる). Decide for both together: keep, or drop てた / てる. (N4 batch 5)

## Lookup coverage

- **`koto-ga-aru-sometimes` (ることがある)**: only found when hovering
  the る of a る-ending verb. In 書くことがある, hovering こと gives
  `koto-nominalizer` instead. A bare ことがある form would fix that but
  also catch たことがある, the "have done before" point. (N4 batch 5)

## Level differences

- **`zurai` (づらい)** and **`naosu` (直す)**: N4 as in the plan (Bunpro);
  bee files both at N3. (N4 batch 5)
- **`gai` (甲斐)**: N4 per the plan; bee's only Bunpro row for かい is the
  question particle かい, a different point. (N4 batch 5)
