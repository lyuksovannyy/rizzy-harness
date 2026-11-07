// Which bundled topic lists suit this user, inferred from their own chat
// titles. It runs in memory in the browser: nothing here is stored or sent,
// and the titles themselves never reach the wall (mix.ts skips any bundled
// line that equals one). English only, like the i18n catalogue.

export type Topic =
  | "software-development"
  | "robotics"
  | "careers"
  | "health"
  | "travel"
  | "money"
  | "food"
  | "science"
  | "language"
  | "home";

export type TitleRow = { title: string; updated_ms: number };

/**
 * What the wall knows of the user's chats. `titles`, the chats the sidebar
 * lists, steer the topics. `banned` is every chat's title, incognito and
 * sub-agent chats included: it only keeps a bundled line that equals one
 * off the wall, so a question asked in private never shows up later.
 */
export type History = { titles: readonly TitleRow[]; banned: readonly string[] };

/** A strong keyword alone matches a topic; a weak one counts half. */
export const KEYWORDS: Record<Topic, { strong: string[]; weak: string[] }> = {
  "software-development": {
    strong: ["code","coding","programming","programmer","bug","debug","debugging","compile","compiler","refactor","api",
      "endpoint","sdk","framework","repo","repository","git","github","gitlab","commit","rebase","merge conflict",
      "pull request","ci","ci/cd","deploy","deployment","docker","dockerfile","kubernetes","k8s","database","sql",
      "postgres","postgresql","mysql","sqlite","mongodb","redis","schema","migration","regex","json","yaml",
      "typescript","javascript","react","vue","svelte","nodejs","node.js","npm","python","rust","cargo","golang",
      "java","kotlin","swift","c++","cpp","c#","dotnet","html","css","frontend","backend","unit test","linter",
      "eslint","webpack","vite","async","segfault","memory leak","stack trace","stacktrace","exception","cli",
      "bash","shell script","terminal","linux","vim","vscode","ide","array","function"],
    weak: ["build","server","error","test","app","script","class","string","loop","query","pipeline","thread","container"] },
  robotics: {
    strong: ["robot","robotic","robotics","ros","ros2","servo","stepper","actuator","gripper","end effector",
      "kinematics","inverse kinematics","arduino","raspberry pi","esp32","stm32","microcontroller","pid","lidar",
      "imu","slam","odometry","drone","quadcopter","rover","manipulator","robot arm","urdf","gazebo","moveit",
      "path planning","motion planning","kalman filter","sensor fusion","pwm","h-bridge","motor driver","brushless","bldc"],
    weak: ["motor","sensor","encoder","torque","autonomous","navigation","controller"] },
  careers: {
    strong: ["job","job search","job hunt","resume","cv","cover letter","interview","interviewing","recruiter",
      "hiring","offer letter","job offer","salary negotiation","promotion","career","linkedin","internship",
      "layoff","laid off","resign","resignation","notice period","two weeks notice","headhunter",
      "job application","career change","performance review"],
    weak: ["manager","boss","coworker","colleague","raise","offer","work","team","portfolio","freelance","apply","salary"] },
  health: {
    strong: ["health","doctor","symptom","headache","migraine","fever","flu","covid","cough","insomnia","nutrition",
      "calorie","protein","vitamin","supplement","workout","fitness","gym","blood pressure","cholesterol",
      "diabetes","anxiety","depression","therapy","therapist","medication","medicine","ibuprofen","paracetamol",
      "allergy","allergies","rash","injury","sprain","back pain","weight loss","heart rate","pregnancy","pregnant",
      "dentist","toothache","diet"],
    weak: ["pain","sleep","tired","energy","healthy","exercise","running","stress","test","cold"] },
  travel: {
    strong: ["travel","traveling","travelling","trip","flight","airport","airline","layover","visa","passport",
      "hotel","hostel","airbnb","itinerary","luggage","baggage","carry on","jet lag","road trip","sightseeing",
      "things to do","vacation","holiday","backpacking","cruise","rental car","car rental","travel insurance",
      "esim","roaming","packing list","day trip"],
    weak: ["beach","tour","train","city","weekend","summer","booking","museum"] },
  money: {
    strong: ["money","budget","budgeting","savings","invest","investing","investment","stock","etf","index fund",
      "bond","crypto","bitcoin","ethereum","retirement","401k","ira","roth","pension","tax","taxes","tax return",
      "deduction","mortgage","loan","interest rate","credit card","credit score","debt","inflation","bank",
      "banking","dividend","net worth","emergency fund","refinance","brokerage"],
    weak: ["price","cost","pay","afford","insurance","rent","salary","portfolio","save"] },
  food: {
    strong: ["recipe","cook","cooking","bake","baking","bread","sourdough","pasta","soup","salad","dinner","lunch",
      "breakfast","meal prep","vegan","vegetarian","gluten free","chicken","beef","pork","tofu","rice","oven",
      "air fryer","instant pot","grill","grilling","sauce","marinade","dessert","cake","cookie","coffee",
      "espresso","tea","wine","ferment","fermentation","substitute for","ingredient"],
    weak: ["eat","food","kitchen","taste","sugar","egg","cheese","knife"] },
  science: {
    strong: ["physics","chemistry","biology","astronomy","astrophysics","planet","black hole","quantum","relativity",
      "evolution","dna","rna","gene","genetics","atom","molecule","periodic table","hypothesis","geology",
      "volcano","earthquake","mathematics","math","maths","equation","calculus","algebra","statistics",
      "probability","theorem","proof","derivative","integral","entropy","photosynthesis","neuron","telescope",
      "nasa","gravity","thermodynamics","speed of light","electron","photon"],
    weak: ["space","star","theory","experiment","cell","energy","climate","element","why"] },
  language: {
    strong: ["translate","translation","meaning of","definition","define","grammar","spelling","pronounce",
      "pronunciation","synonym","antonym","word for","idiom","verb","noun","adjective","conjugate","conjugation",
      "tense","plural","proofread","punctuation","etymology","spanish","french","german","japanese","chinese",
      "mandarin","korean","italian","portuguese","latvian","russian","ukrainian","arabic","hindi","esl","ielts",
      "toefl","duolingo","in english"],
    weak: ["english","word","phrase","sentence","rewrite","essay","letter","accent"] },
  home: {
    strong: ["house","apartment","landlord","lease","cleaning","laundry","stain","mold","mould","plumbing","plumber",
      "leak","leaking","faucet","toilet","drain","clogged","heater","boiler","thermostat","hvac","air conditioner",
      "drywall","furniture","ikea","garden","gardening","houseplant","lawn","renovation","renovate","diy","drill",
      "shelf","shelves","smart home","vacuum","dishwasher","washing machine","fridge","refrigerator","pest",
      "mice","cockroach","interior design","decorate","decorating","moving house","roof","gutter","insulation"],
    weak: ["home","room","kitchen","wall","paint","painting","rent","clean","plant","tool","window"] },
};

export const TOPICS = Object.keys(KEYWORDS) as Topic[];

/**
 * The words of a title and its two- and three-word phrases, lowercased and
 * without accents ("résumé" is "resume"); a plural also counts as its
 * singular. Code-ish tokens keep their marks: c++, c#, node.js, ci/cd.
 */
export function grams(title: string): Set<string> {
  const words = title
    .toLowerCase()
    .normalize("NFKD")
    .replace(/\p{M}/gu, "")
    .replace(/[^a-z0-9+#.\s/-]/g, " ")
    .split(/\s+/)
    .map((w) => w.replace(/\.+$/, ""))
    .filter(Boolean);
  const out = new Set<string>();
  words.forEach((w, i) => {
    const forms = w.length > 3 && w.endsWith("s") ? [w, w.slice(0, -1)] : [w];
    for (const f of forms) {
      out.add(f);
      if (i > 0) out.add(`${words[i - 1]} ${f}`);
      if (i > 1) out.add(`${words[i - 2]} ${words[i - 1]} ${f}`);
    }
  });
  return out;
}

/** The topics a title is about: a strong keyword, or two weak ones. */
export function matchTopics(title: string): Topic[] {
  const g = grams(title);
  const has = (k: string) => g.has(k);
  return TOPICS.filter((t) => KEYWORDS[t].strong.filter(has).length + 0.5 * KEYWORDS[t].weak.filter(has).length >= 1);
}

const DAY_MS = 86_400_000;

/**
 * How much of the wall each topic gets. `m` counts the titles that matched
 * any topic (mix.ts turns it into the share of topic lines); `p` gives each
 * topic that at least two titles matched its part of that share. Recent
 * chats count more, a title about k topics gives each 1/k, and the power
 * of 0.75 keeps one busy topic from taking the whole wall.
 */
export function topicWeights(titles: readonly TitleRow[], now: number): { m: number; p: Map<Topic, number> } {
  const score = new Map<Topic, number>();
  const count = new Map<Topic, number>();
  let m = 0;
  for (const row of titles) {
    const ts = matchTopics(row.title);
    if (!ts.length) continue;
    m++;
    const age = Math.max(0, now - row.updated_ms) / DAY_MS;
    const w = Math.max(0.15, 0.5 ** (age / 30));
    for (const t of ts) {
      score.set(t, (score.get(t) ?? 0) + w / ts.length);
      count.set(t, (count.get(t) ?? 0) + 1);
    }
  }
  const eligible = [...score].filter(([t]) => (count.get(t) ?? 0) >= 2).map(([t, s]) => [t, s ** 0.75] as const);
  const sum = eligible.reduce((a, [, s]) => a + s, 0);
  return { m, p: new Map(eligible.map(([t, s]) => [t, s / sum])) };
}
