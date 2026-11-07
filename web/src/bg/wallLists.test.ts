// What the Ask wall may show. The wall is decoration on the home screen,
// so a line that reads as the app taking a position, or as something
// personal, has no place on it: bodily functions, drugs and medication,
// illness, belief, identity, politics, crime and gossip about real people.
// The bundled lists were curated against this; it keeps them that way.
import { describe, expect, test } from "vitest";
import { parseList } from "./wallData";

const lists = import.meta.glob<string>("../../public/wall/**/*.txt", { query: "?raw", import: "default", eager: true });

const words = (...w: string[]) => new RegExp(`\\b(?:${w.join("|")})\\b`, "i");
const DENY: Record<string, RegExp> = {
  body: words(
    "poop\\w*", "pee", "urine", "farts?", "farting", "burps?", "diarrh\\w*", "constipat\\w*", "nause\\w*", "throw up",
    "vomit\\w*", "sweat\\w*", "armpits?", "nipples?", "periods?", "ovulat\\w*", "pregnan\\w*", "breastfeed\\w*",
    "induce labor", "milk supply", "boogers?", "gassy", "hemorrhoids?", "menstru\\w*", "\\w*menopause", "c section",
    "diva cup", "testosterone", "kegels?", "bra size", "spit up", "uti", "bv", "chlamydia", "tonsils?", "skin tags",
    "blackheads", "calluses", "toenails?", "dry socket", "bad breath", "jock itch", "scabies", "warts", "maggots",
    "rocky mountain oysters", "fat", "lose weight", "weight loss", "jowls", "hip dips", "stomach", "kiss", "my tongue",
  ),
  drugs: words(
    "xanax", "gabapentin", "zoloft", "lexapro", "prednisone", "doxycycline", "famotidine", "jardiance", "ondansetron",
    "zofran", "naproxen", "zepbound", "glp-?1s?", "benzos", "barbiturates", "quaaludes", "kratom", "kava", "dmt",
    "edibles", "pills?", "nootropics", "statins", "beta blockers", "nsaids", "when high", "vap(?:e|ing)", "smoking",
    "zyns?", "pouches", "reta", "prescri\\w*", "birth control", "chs",
  ),
  illness: words(
    "cancer", "lupus", "lymph\\w*", "leprosy", "epilepsy", "schizophren\\w*", "psychosis", "bipolar", "bpd", "ocd",
    "crohn'?s", "cellulitis", "copd", "diverticulitis", "endometriosis", "eds", "fibromyalgia", "gout", "gerd", "hpv",
    "hs", "ibs", "impetigo", "jaundice", "meningitis", "mono", "ms", "als", "cte", "narcolepsy", "neuropathy",
    "osteoporosis", "pcos", "pmdd", "preeclampsia", "palliative", "plantar fasciitis", "rhabdo\\w*", "rsv", "ringworm",
    "rhinovirus", "sepsis", "apnea", "shingles", "colitis", "vertigo", "bronchitis", "botulism", "aids", "measles",
    "polyps", "fibroids", "cysts?", "\\w*stones", "fluoride", "canker", "cholesterol", "blood pressure", "hypertension",
    "triglycerides", "blood tests?", "wbc", "mchc", "rdw", "bun", "egfr", "ferritin", "cortisol", "insulin",
    "sore throat", "appendi\\w*", "adhd", "anemia", "diabetes", "symptoms?", "strep", "covid", "dengue",
    "hand foot and mouth", "depression", "dehydration", "blood clot", "celiac", "disease", "disorder", "\\w*glycemi\\w*",
    "autis\\w*", "neurodivergent", "intrusive thoughts", "floaters", "varicose", "bunions", "leukocytes", "neutrophils",
    "eosinophils", "platelets", "pinched nerve", "carpal tunnel", "diastasis recti", "heartburn", "palpitations",
    "headaches?", "dizzy", "deadly", "dies?", "kill(?:s|ed)?", "hospital", "surgery", "pancrea\\w*", "ulcers?", "hives",
    "bruise", "nosebleeds", "swell", "hurts?", "aches?", "pots", "emdr", "cataracts", "keloids",
  ),
  belief: words(
    "jews?", "jewish", "muslims?", "islam\\w*", "sharia", "christian\\w*", "catholic\\w*", "rosary", "pray\\w*", "bible",
    "nuns?", "mormons?", "jehovah", "quakers", "scientology", "amen", "alhamdulillah", "hallelujah", "agnostic",
    "zionis[mt]s?", "commandments", "deadly sins", "yom kippur", "rosh hashanah", "diwali", "dhanteras", "halal",
    "kosher", "freemasons", "angel numbers", "yamakas",
  ),
  identity: words(
    "gypsies", "irish twins", "lesbians?", "gay", "pansexual", "non binary", "cisgender", "furries", "juggalos",
    "hypergamy", "woke", "dei", "xenophobia", "old people", "blind people", "wheelchair", "bald", "fag\\w*", "racis\\w*",
  ),
  politics: words(
    "vote", "voting", "elections?", "ballots?", "midterms?", "veto", "filibuster", "impeach\\w*", "gerrymander\\w*",
    "socialism", "communism", "fascism", "liberals?", "gop", "dsa", "aipac", "cair", "palestine", "israel", "netanyahu",
    "iran", "war", "hegseth", "newsom", "abbott", "beshear", "roy cooper", "talarico", "crockett", "leavitt",
    "susie wiles", "beautiful bill", "electoral", "president\\w*", "senators?", "prop \\d+", "vouchers", "medicaid",
    "food stamps", "brotherhood", "rioting", "riots?", "el sayed", "(?:mississippi|alaska) taking so long",
    "not called yet", "political",
  ),
  harm: words(
    "jail", "prison", "murder\\w*", "shooter", "crime", "suspended", "court", "bankruptcy", "in trouble", "in debt",
    "divorc\\w*", "accidents?", "hazing", "cancelled", "rico", "love bombing", "gaslighting", "narcissis\\w*",
    "life so hard", "mean to me", "sex\\w*",
  ),
};

const denied = (line: string) => Object.keys(DENY).filter((k) => DENY[k]!.test(line));

describe("wall lists", () => {
  const entries = Object.entries(lists).map(([path, text]) => [path.replace(/^.*\/wall\//, ""), parseList(text)] as const);

  test("the general list and all ten topic lists are bundled", () => {
    expect(entries.map(([name]) => name).sort()).toEqual([
      "queries.txt",
      ...["careers", "food", "health", "home", "language", "money", "robotics", "science", "software-development", "travel"].map(
        (t) => `topics/${t}.txt`,
      ),
    ]);
  });

  test.each(entries)("%s: nothing the wall must not show", (_, lines) => {
    expect(lines.filter((l) => denied(l).length).map((l) => `${l} (${denied(l).join(", ")})`)).toEqual([]);
  });

  // Enough left to fill the widest wall (64 lines) many times over, and
  // each topic's share of it (at most 60%).
  test.each(entries)("%s: still plenty to draw from", (name, lines) => {
    expect(lines.length).toBeGreaterThanOrEqual(name === "queries.txt" ? 700 : 60);
  });

  test("the policy catches what it is meant to", () => {
    for (const s of ["why do farts smell", "what are xanax prescribed for", "what are jews", "why is netanyahu called bibi", "why is pancreatic cancer so deadly", "how to vote in the us", "why is diddy in jail"]) {
      expect(denied(s), s).not.toEqual([]);
    }
    for (const s of ["why is the sky blue", "why is gas so high", "diy ant killer", "how to tie a tie", "how to remove hard water stains from glass", "how to make a killing"]) {
      expect(denied(s), s).toEqual([]);
    }
  });
});
