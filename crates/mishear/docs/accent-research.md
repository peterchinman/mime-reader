<!-- Research notes behind src/accents.rs and the vowel chart in src/deform.rs.
     Compiled from the sources listed in section 4; the working files it
     mentions under /tmp are not part of the repo. -->

# Accent deformation rules over CMUdict ARPAbet: research notes

Compiled 2026-11-07. Covers part 1 (accent rule sets), part 2 (vowel-space coordinates) and part 3 (shifts as geometry) of the brief. Source keys such as [HibE] are expanded with URLs in section 4.

---

## 0. Conventions, caveats, CMUdict facts

**Notation**
- `X → Y / L _ R`: rewrite X as Y when L comes before it and R after it. `#` = word edge, `∅` = deletion, `+X` = insert X.
- V = any vowel. V1, V2, V0 = a vowel with that stress digit. Vs = a stressed vowel (1 or 2). C = any consonant.
- **coda / post-vocalic**: CMUdict has no syllable boundaries, so "coda" means a consonant that follows a vowel and is not followed by a vowel in the same word (`V _ C` or `V _ #`). A "closed syllable" is a vowel followed by `C C` or `C #`.
- Classes: STOP_VL = P T K; STOP_VD = B D G; FRIC_VL = F TH S SH; FRIC_VD = V DH Z ZH; AFFR = CH JH; NASAL = M N NG; LIQ = L R; VOICELESS = P T K F TH S SH CH; LABIAL = P B F V M W.
- An inserted vowel gets stress 0. A substituted vowel keeps the stress digit of its input.
- **Sal(ience) 1–3** (3 = a GA listener could name the accent from this feature alone). **These numbers are my own judgement.** I based them on which features the sources call stereotyped or salient; they are not measured.
- **Type**: P = phonological (fires wherever the context matches). L = lexical (applies to certain words only). Each L rule gives a phoneme-context approximation and says how lossy it is.
- **NE** = not expressible in the 39-symbol set. Where it helps, I give the closest audible substitute.
- I cite the underlying author (Wells 1982, Hickey 2004, ...) when the Wikipedia article attributes the claim to them. **I have not checked those books directly.** Items marked **[unverified]** are widely repeated, but I found no source I could read.

**CMUdict facts that affect these rules** (checked against `data/CMUdict/cmudict-0.7b` in this repo)
- AA covers LOT, PALM and START: `LOT L AA1 T`, `FATHER F AA1 DH ER0`, `CAR K AA1 R`. A rule meant for LOT also hits PALM words unless it excludes `AA R` and a PALM word list.
- THOUGHT is split inconsistently between AO and AA: `THOUGHT TH AO1 T`, `LAW L AO1`, but `CAUGHT K AA1 T` (alternate `CAUGHT(1) K AO1 T`), `BOUGHT B AA1 T`, `ON AA1 N`. CLOTH words use AO (`CLOTH`, `OFF`, `DOG`, `LONG`). NORTH and FORCE are both `AO R`.
- There is no yod after coronals (`TUNE T UW1 N`, `NEW N UW1`). Yod insertion (RP "nyoo") and yod coalescence (Irish "choon") can't tell TUNE from TWO, so they need a word list or the spelling.
- There is no `HH W` for wh-words (`WHICH W IH1 CH`, `WHAT W AH1 T`); only names such as `JUAN(1)` have it. Any wh/w distinction needs the spelling.
- Every NURSE word is ER (BIRD, HERD, FERN, TERM, TURN, GIRL, WORD, EARN). The Irish and Scottish NURSE splits need the spelling.
- Weak vowels: `ROSES R OW1 Z IH0 Z` vs `ROSA'S R OW1 Z AH0 Z`. The happY vowel is IY0 (`HAPPY HH AE1 P IY0`).

**Symbols the 39-phoneme set lacks.** Extended ARPAbet has DX (flap), Q (glottal stop), WH (voiceless labial-velar), AX (schwa), AXR, IX (close central unrounded), UX (close central rounded), EL/EM/EN (syllabic consonants) and NX (nasal flap) [ARPA]. Suggestion: have rules write to this extended tier (Cockney T → Q, Irish slit-t → a private symbol), then collapse to the 39 for matching through one table: Q → ∅ (or T), DX → T/D (a GA listener merges them), WH → W, UX → UW, IX → IH0, AX → AH0, AXR → ER0, EL/EM/EN → AH0 + L/M/N. That keeps each rule faithful and puts all the lossiness in one place.

**Order of application**
1. Lexical rules (BATH, MEAT–MATE).
2. Vowel chain shifts, applied **simultaneously** (a single pass over the original vowels). Applied in sequence, NZ AE→EH followed by EH→IH would send TRAP all the way to KIT.
3. R-deletion (non-rhotic accents), before any rule that looks at "V before R".
4. Consonant rules.
5. Epenthesis.

---

## 1. Accent rule sets

### 1.1 Irish (Hiberno-English): priority section

Irish accents vary widely. Following Hickey, [HibE] separates Ulster, West and South-West, local Dublin, advanced ("new") Dublin and supraregional Irish. Stage Irish is a stereotype that grew out of 16th- to 18th-century English drama. Paraphrasing Hickey's abstract: some of its features have vanished, some survive only in certain varieties and contexts, and some persist in general Irish English [Hickey2010]. Shakespeare's Captain Macmorris ("What ish my nation?") is the usual starting point [StageIr] [Hickey2010].

Variety tags: **ST** = stage/stereotype, **SR** = supraregional (standard), **WSW** = West and South-West / rural, **LD** = local Dublin, **AD** = advanced Dublin, **UL** = Ulster, **CONS** = conservative/older speakers.

| ID | Feature | Rule | Variety | Sal | Type | Notes / lossiness | Src |
|---|---|---|---|---|---|---|---|
| IRL-01 | TH-stopping, voiceless | TH → T (all positions) | ST, WSW, LD; SR/AD use dental [t̪] | 3 | P | In WSW and LD the two merge (thin = tin). In SR and AD, [t̪] stays distinct from [t]. GA listeners can identify the dental/alveolar contrast, but less accurately than Irish listeners, and worse word-finally [Kelly2017]. For SR, apply with probability <1 or only word-initially. | [HibE] [ThStop] [Kelly2017] |
| IRL-02 | TH-stopping, voiced | DH → D | as IRL-01 | 3 | P | then = den | [HibE] |
| IRL-03 | MEAT–MATE: "tay" for *tea* | IY → EY in historical ME ɛː words (tea, meat, beat, leaf, beak, peace, reason, sea) | ST; CONS/rural | 3 (ST), 1 (real) | L | **No phoneme context works**, because CMUdict IY merges *meet* and *meat*. Best approximation: if the word is spelled ‹ea› and CMUdict has IY, output EY. Spelled ‹ea› words without IY (head, great, bear) fall outside the IY condition, so the heuristic is fairly clean. It misses non-‹ea› stage forms ("Jaysus"). In real speech only rural or conservative varieties keep *meat* ≠ *meet* (Wells 1982 via [HFV]). Mid-Ulster has a near-merger (beat/bait, peace/pace, reason/raisin) [Maguire]. "tay", "Jaysus" and O'Casey's *Juno and the Paycock* are cited as examples [Maguire]. | [HFV] [Maguire] [Wikt-tay] |
| IRL-04 | "Oirish": PRICE → CHOICE | AY → OY | ST only | 3 | P | Caricature. "Oirish" is a pronunciation spelling that mimics the accent [Wikt-Oirish]. The real PRICE vowel is centralised or backed, not [ɔɪ]: WSW [æɪ~ɐɪ], LD [əɪ~ɐɪ], AD [ɑɪ~ɐɪ], SR [aɪ~ɑɪ] [HibE]. Those real onsets are NE; the closest real substitute is AY unchanged. | [Wikt-Oirish] [HibE] |
| IRL-05 | CHOICE unrounded | OY → AY (boil ≈ bile, join ≈ "jine") | WSW [əɪ~ɑɪ], LD [aɪ~äɪ] | 2 | P | Real feature [HibE vowel table]. Together with IRL-04 it pushes PRICE and CHOICE toward each other. The spelling "jine" is **[unverified]**. | [HibE] [SWIrE] |
| IRL-06 | "ould" for *old* | OW → AW / _ L D | ST, colloquial | 2 | L | "ould" /aʊld/ ~ /aʊl/ is a colloquial Irish pronunciation spelling of *old* [Wikt-ould]. Extending it to cold, told, hold, sold is my generalisation (**[unverified]**), so it is lossy. | [Wikt-ould] |
| IRL-07 | STRUT rounded, no FOOT–STRUT split | AH1/AH2 → UH (luck → "look") | LD [ʊ]; SR/WSW/UL [ʌ̈~ʊ]; AD [ɤ~ʊ] | 2 | P | STRUT is "centralised and often rounder" than in GA [HibE]. The full shift to UH fits LD. For SR use probability <1. | [HibE] [DubE] |
| IRL-08 | Two NURSE classes | ER1 → UH1 R / {P B F V M W T D} _ ; otherwise ER1 → EH1 R | LD, WSW, UL, CONS | 2 | L | Source classes: /ɛr/ in earn, girl, earth vs /ʊr/ in urn, work, first, bird. /ʊr/ covers ‹ur›, ‹or›, and ‹ir› after alveolar stops or labials; *prefer* and *per* are /ɛr/ exceptions (Wells 1982:421). The context approximation misfiles fern and term (‹e› words). A spelling-based rule is better: ‹ur›/‹or›, or ‹ir› after a labial or T/D → UH R; ‹er›/‹ear› → EH R. AD and SR merge NURSE as [ɚː], so no change there. | [HibE] |
| IRL-09 | Epenthetic schwa between sonorants | +AH0 / {L R} _ {M N} (coda): film → F IH1 L AH0 M, form → F AO1 R AH0 M | general (Irish-influenced) | 2 | P | The source says "between sonorants" and gives film and form as examples. Whether R+N (*born*) qualifies is **uncertain**; Scotland has it (see SCO-07). Adds a syllable, so it changes which words match. | [HibE] |
| IRL-10 | /s/ → [ʃ] before a consonant | S → SH / _ C (fist → "fished", castle → "Cashel", arrest → "arresht"). For ST, also S → SH / _ # in *is* ("ish") | WSW, very conservative; ST | 2 | P | Wells 1982:425 via [SWIrE]. Macmorris's "ish" is the stage form [StageIr]. | [SWIrE] [HibE] [StageIr] |
| IRL-11 | pin–pen merger | EH → IH / _ {M N} | South-West (Cork, Kerry, Limerick) | 1 | P | Formerly widespread in Ireland and not limited to nasal contexts (Wells 1982:423 via [HFV]) | [SWIrE] [HFV] |
| IRL-12 | MOUTH backed/rounded | AW → OW ("about" near "a boat") | South-West, rural | 2 | P | [ɐʊ~ʌʊ~ɔʊ]. OW is the nearest ARPAbet vowel; the real onset is NE. | [SWIrE] |
| IRL-13 | LOT/THOUGHT unrounded and open | AO → AA | WSW, LD [ä] | 1 | P | Many GA speakers already merge the two, and CMUdict is inconsistent here anyway (see section 0). | [HibE] [SWIrE] [DubE] |
| IRL-14 | START fronted | AA → AE / _ R ("car" with a TRAP-like vowel) | South-West [æːɹ], LD [æːɹˠ~ɛːɹˠ] | 2 | P | AD backs START instead ([ɑːɻ]) | [SWIrE] [DubE] |
| IRL-15 | FACE and GOAT monophthongs [eː], [oː] | NE: keep EY, OW | WSW, UL, SR (FACE) | 2 | — | A GA listener still hears EY/OW. No 39-symbol substitute. LD GOAT [ʌʊ] could be OW → AW (lossy). | [HibE] |
| IRL-16 | any/many with TRAP | EH → AE in *any*, *many* | general | 1 | L (exact, 2 words) | They rhyme with Annie and Danny | [HibE] |
| IRL-17 | gave/came with DRESS | EY → EH in *gave*, *came* | some speakers | 1 | L | The source marks this "citation needed" | [HibE] |
| IRL-18 | -ing → -in' | IH0 NG → IH0 N / _ # | ST, colloquial | 2 | P | Wells: G-dropping occurs "wherever there is an English-speaking working class", except South Africa [Gdrop]. **I found no Irish-specific statement.** | [Gdrop] |
| IRL-19 | Slit-t [θ̠] | NE | SR, WSW, AD (not UL) | 2 | — | Intervocalic and syllable-final /t/ become a "slit fricative", like /s/ without the hiss (Hickey 1984 via [HibE]). The closest ARPAbet output is T unchanged. S or TH would be a speculative, low-weight alternative (my inference). On the extended tier, mark it as a private symbol. | [HibE] |
| IRL-20 | T-glottalling / loss | T → ∅ / V _ # (also V _ V0) | LD ([ʔ], [h] or ∅) | 1 | P | The glottal stop is NE. Deletion is the GA-audible approximation. | [HibE] [DubE] |
| IRL-21 | Cluster reduction | D → ∅ / N _ #; T → ∅ / S _ # (find → "fine", first → "firs'", last → "las'") | LD | 1 | P | "poun(d)", "las(t)" (de Gruyter 2004 via [HibE]) | [DubE] [HibE] |
| IRL-22 | Vowel breaking in closed syllables | IY1 → IY1 AH0, UW1 → UW1 AH0, AY1 → AY1 AH0, AW1 → AW1 AH0 / _ C (C or #) | LD | 1 | P | [ijə], [uwə], [əjə], [ɛwə]. Adds a syllable. | [DubE] |
| IRL-23 | Clear (non-velarised) L | NE | general | 1 | — | ARPAbet L has no velarisation feature. No substitute. | [HibE] |
| IRL-24 | wh = [ʍ] kept | W → HH W in wh-words | WSW, LD, CONS | 1 | L | Needs the spelling (CMUdict lacks HH W). HH W is a lossy rendering of [ʍ]. Merging in SR and AD. | [HibE] |
| IRL-25 | "haitch" | the letter name *aitch* → HH EY1 CH | Republic of Ireland | 1 | L | [HibE] | [HibE] |
| IRL-26 | Yod-coalescence | T UW → CH UW, D UW → JH UW (tune → "choon", duty → "jooty") | general | 1 | L | **Highly lossy as a phoneme rule** (see section 0). Use a word list or the spelling (‹u›, ‹ew›, ‹ue› after t/d). Irish also drops the yod after N, L, S, as GA does. | [HibE] |
| IRL-27 | /h/ dropped before /j/ | HH Y → Y (hue → "you") | South-West | 1 | P | [SWIrE] | [SWIrE] |
| IRL-28 | Ulster package | KIT lowered [ë~ɘ]: IH → EH (lossy). No happY tensing: IY0 → EY0 / _ #. Traditional palatalised K/G before open front vowels: K → K Y, G → G Y / _ AE. PRICE [ɛɪ], GOOSE/FOOT [ʉ] and rising statement intonation are NE. | UL | 1 each | P | Ulster is "not stage Irish". It keeps [θ ð] and flaps T [Flap]. | [HibE] [Brogue] |
| IRL-29 | Rhoticity | none needed (Irish is rhotic) | SR, most | — | — | Louth, Cavan and traditional LD are non-rhotic; modern Dublin is rhotic [Rhot]. For an old-Dublin profile, reuse CKN-07. | [Rhot] [HibE] |
| IRL-30 | Dublin 4 ("Dortspeak") rounding | AA (START) and BATH-set AE → AO ("a glass in the bar" [ə glɒːs ɪn ðə bɒːɹ]) | D4, 1980s–90s, now unfashionable | 1 | L/P | Use the BATH approximation from RP-03 | [DubE] |
| IRL-31 | Intonation: SW "sing-song" (high pitch, then a sharp drop on stressed long vowels); Ulster rising statements | NE | SW, UL | 2 | — | Not segmental | [SWIrE] [Brogue] |
| IRL-32 | Weak-vowel merger | IH0 → AH0 | almost universal | 1 | P | Wells 1982 via [HFV] | [HFV] |
| IRL-33 | "me" for possessive *my* | AY → IY in unstressed *my* | ST, colloquial | 2 | L | **[unverified]**: I found no source. Also common in British vernaculars. | — |

**Stage-Irish profile for a GA listener.** IRL-01, 02, 03 (the ‹ea› heuristic), 04, 06, 07, 09, 10 (including "ish"), 18 and 33, plus IRL-05 and 12 at low weight. A "realistic rural/West" profile drops IRL-04 and 33 and adds 05, 11, 12, 13, 14 and 15 (no-op). A "supraregional" profile keeps only IRL-01/02 at reduced probability, 07 (partial) and 09.

### 1.2 Scottish English

| ID | Feature | Rule | Sal | Type | Notes / lossiness | Src |
|---|---|---|---|---|---|---|
| SCO-01 | No FOOT/GOOSE distinction | UH → UW (pull = pool) | 2 | P | Scots /u/ realised [ʉ~u~ʏ]. Fronting is NE. | [ScotE] |
| SCO-02 | LOT = THOUGHT, rounded [ɔ] | AA → AO (LOT only: not before R, excluding PALM words) | 1 | P | Most Central Scottish varieties [ScotE]. Lossy because of the CMUdict AA problem. | [ScotE] |
| SCO-03 | TRAP = BATH = PALM, [a] | NE: leave AE and AA | 1 | — | No single ARPAbet vowel covers [a] | [ScotE] |
| SCO-04 | FACE [e(ː)], GOAT [o(ː)] | NE: keep EY, OW | 2 | — | as IRL-15 | [ScotE] |
| SCO-05 | Tapped [ɾ], rarely trilled /r/ | NE. Caricature substitute: R → D / V _ V0 | 3 (caricature "rolled R") | P | The substitute is my inference: GA speakers' own [ɾ] belongs to /t, d/, so an intervocalic tap is likely to be heard as D. | [ScotE] |
| SCO-06 | Three-way NURSE | ER → IH R (bird, fir), AH R (curse, fur), EH R (herd, fern; [ɛ̈r] in earth, jerk) | 2 | L | Needs the spelling: ‹ir›/‹yr› → IH R, ‹ur›/‹or› → AH R, ‹er›/‹ear› → EH R. Spelling-based rule is low-loss; there is no phoneme-only version. | [ScotE] |
| SCO-07 | Epenthesis | +AH0 / R _ L (girl, world become two syllables), R _ M, R _ N, L _ M (film) | 2 | P | Adds a syllable | [ScotE] |
| SCO-08 | wh = [ʍ] | W → HH W in wh-words | 1 | L | Needs the spelling | [ScotE] |
| SCO-09 | /x/ (loch, Gaelic/Scots words, some speakers' *technical*) | NE. Closest: K (the GA "lock") or HH | 2 | L | Wells 1982:408 via [ScotE] | [ScotE] |
| SCO-10 | happY = FACE | IY0 → EY0 / _ # | 2 | P | "most commonly /e/" [ScotE] | [ScotE] |
| SCO-11 | -ed → /t/ after an unstressed vowel | D → T / V0 _ # (ended [ɛndɪt], carried [karɪt]) | 1 | P | Lossy: also fires on monomorphemic -d (method, salad) | [ScotE] |
| SCO-12 | T-glottalling (colloquial) | T → ∅ / V _ V0 and V _ # (butter [ˈbʌʔər]) | 2–3 (Glasgow) | P | The glottal stop is NE; deletion is the GA-audible approximation | [ScotE] |
| SCO-13 | /θ/ → [h] "in some contexts" | TH → HH (low weight) | 1 | P | The source gives no contexts | [ScotE] |
| SCO-14 | -in' | IH0 NG → IH0 N | 1–2 | P | Same colloquial speakers as SCO-12 | [ScotE] |
| SCO-15 | KIT near [ʌ] after /w/ | IH → AH / {W, HH W} _ | 1 | P | The source marks this "citation needed" | [ScotE] |
| SCO-16 | Dark L everywhere; SVLR vowel length | NE | — | — | Length is not represented | [ScotE] |
| SCO-17 | **Scots layer** (broad Scots words, not Scottish Standard English) | AW → UW (house → *hoose*, down → *doun*, now → *nou*); OW → EY in OE ā words (home → *hame*, stone → *stane*, bone → *bane*); EH → IY (head → *heid*, dead → *deid*); UH → IH (good → *guid*, northern); AO L → AO (all → *aw*, ball → *baw*); "no" → *nae* | 3 (caricature) | L | AW → UW over all AW is a good approximation, because most modern /aʊ/ comes from ME/OE ū. The others need word lists. *oot* and *aboot* are not listed in the source; their Scots forms are **[unverified]**. This layer is effectively a partial **reverse Great Vowel Shift** (section 3). | [Scots] |

### 1.3 Cockney / London

| ID | Feature | Rule | Sal | Type | Notes / lossiness | Src |
|---|---|---|---|---|---|---|
| CKN-01 | H-dropping | HH → ∅ / # _ | 3 | P | Also general English weak forms (he, him, her, have) [Hdrop]. Sivertsen treats [h] as partly an emphasis marker. | [Cockney] [Hdrop] |
| CKN-02 | TH-fronting | TH → F (thin [fɪn], maths [mæfs]) | 3 | P | | [Cockney] [ThFront] |
| CKN-03 | DH-fronting, non-initial | DH → V / V _ (bother → "bovver", brother, father) | 3 | P | Not in *the*, *they*, *them*, *their* [ThFront] | [Cockney] [ThFront] |
| CKN-04 | Initial DH stopping | DH → D / # _ (they [dæj]) | 2 | P | Wells 1982:329 | [Cockney] |
| CKN-05 | T-glottalisation | NE (Q). Substitute: T → ∅ / V _ V0, V _ #, V _ {L M N}0 (butter, Clapham [ˈkl̥æʔm̩], bottle) | 3 | P | Less often for /k/ and /p/. Deletion creates hiatus, so match it fuzzily. | [Cockney] |
| CKN-06 | L-vocalisation | Coda L → UH0 after front/central vowels; coda L → ∅ after AO, OW, UW, UH (Millwall [ˈmɪwwɔw]) | 2–3 | P | Realised [u ʊ o ɤ]. Merges vowels before L (real = reel = rill [ɹɪw]) [Lvoc]. ARPAbet W can't sit in a coda, hence UH0. | [Cockney] [Lvoc] |
| CKN-07 | Non-rhotic | R → ∅ / V _ (C or #); ER0 → AH0 (broad [ɐ]: AA0, "buttah"); ER1 → NE ([ɜː]), closest AH1 | 3 | P | See RP-01 for the centring-diphthong outputs | [Cockney] [Rhot] |
| CKN-08 | Linking and intrusive R | +R / {AA AO AH0 ER→AH0} # _ V (law-r-and order) | 1–2 | P | Across word boundaries. The [Cockney] article doesn't cover it; [Rhot] notes Cockney /r/-insertion. | [Rhot] |
| CKN-09 | FACE lowered | EY → AY (bait [bæjʔ]; mate ≈ "mite") | 3 | P | [æj~aj] (Wells 1982:307–8). Part of the Cockney chain iː → eɪ → aɪ → ɔɪ [Chain]: apply CKN-09, 10 and 13 simultaneously. | [Cockney] [Chain] |
| CKN-10 | PRICE backed | AY → OY in broad speech ([ɒj]); otherwise [ɑj], unchanged | 2–3 | P | | [Cockney] |
| CKN-11 | GOAT onset lowered | OW → AW (coat [kʰɐɰʔ]; no ≈ "now") | 3 | P | [æ̈ɰ~ɐɰ]. AW is the closest available vowel; lossy. | [Cockney] |
| CKN-12 | MOUTH monophthong [æː] | AW → AE (house → "hass"); alternative AW → AA | 3 | P | Also [æw] | [Cockney] |
| CKN-13 | FLEECE [əj] | NE. Caricature: IY → EY at low weight | 1 | P | The first step of the [Chain] Cockney shift. The GA percept is uncertain. | [Cockney] [Chain] |
| CKN-14 | GOOSE [əʉ~ʉː] | NE | 1 | — | | [Cockney] |
| CKN-15 | BATH broad A | AE → AA (see RP-03) | 2 | L | | [Cockney] [TrapBath] |
| CKN-16 | -in' | IH0 NG → IH0 N or AH0 N (taking [ˈtæɪkən]) | 2 | P | The *-thing* suffix is also affected | [Gdrop] |
| CKN-17 | Final THOUGHT [ɔə~ɔwə] | AO → AO W AH0 / _ # | 1 | P | Adds a syllable | [Cockney] |
| CKN-18 | Yod-coalescence (tune [tʃʰʉwn]) | as IRL-26 | 1 | L | | [Cockney] |

### 1.4 RP / Southern British (modern SSBE unless marked)

| ID | Feature | Rule | Sal | Type | Notes / lossiness | Src |
|---|---|---|---|---|---|---|
| RP-01 | Non-rhotic | R → ∅ / V _ (C or #), with these outputs: IH R → IH AH0 (NEAR [ɪə], modern [ɪː]); EH R → EH (SQUARE [ɛː]); UH R → AO (CURE merged with THOUGHT for most speakers); AA R → AA; AO R → AO; ER0 → AH0; ER1 → NE ([ɜː]), closest AH1 or ER1 kept | 3 | P | father = farther, caught = court (Wells 1982) | [RP] [Rhot] |
| RP-02 | Linking and intrusive R | +R across # before a vowel ("idea-r-of") | 1–2 | P | Stigmatised, but common in RP | [Rhot] |
| RP-03 | BATH (trap–bath split) | AE1 → AA1 / _ {F, TH, S}, _ N T, _ N S, _ N CH, _ M P AH0 L (bath, path, grass, last, after, laugh, half, calf, ask, aunt, can't, dance, chance, plant, example, castle) | 3 | L | **Lexically irregular**: frequent words and monosyllables are likelier to have it, and it is rare in open syllables. Keep an exception list (gas, mass, maths, ant, cant, *passage*; *graph* varies). As a pure context rule it over-applies noticeably to rarer words. `CAN'T K AE1 N T` becomes `K AA1 N T`. | [TrapBath] [RP] |
| RP-04 | LOT rounded [ɒ] | AA → AO for LOT (not before R, excluding PALM words such as father, palm, spa, calm, bra) | 2 | P | Modern RP has moved toward [ɔ]. The CMUdict AA problem makes this lossy. CLOTH words are already AO, so no change there. | [RP] [AmBr] |
| RP-05 | GOAT [əʊ] | NE: keep OW | 3 | — | Very salient to GA ears, but ARPAbet has no central-onset diphthong. Before dark /l/ the onset is backer [ɒʊ]. | [RP] |
| RP-06 | THOUGHT raised [oː] | NE. Strong setting: AO → OW (lossy, my inference) | 1 | — | | [RP] |
| RP-07 | No T-flapping | No rewrite. When matching, **don't** merge intervocalic T and D (water, butter, latter ≠ ladder) | 2–3 | P | Flapping is "not used very often" in RP | [RP] [Flap] |
| RP-08 | Yod retention (conservative) | +Y / {T D N S L} _ UW in historically yod words (tune → T Y UW N, new → N Y UW) | 2 | L | Needs a word list (see section 0). Modern RP coalesces instead: tube → CH, dew → JH. | [RP] |
| RP-09 | Smoothing | AY ER0 → AA (tire, fire); AW ER0 → AA (tower); EY ER0 → EH (layer) | 1 | P | Casual speech | [RP] |
| RP-10 | Tapped /r/ (traditional RP: very [veɾɪ]) | NE. Substitute: R → D / V _ V0 ("veddy") | 2 (posh caricature) | P | Largely gone in modern RP. The D substitute is my inference (see SCO-05). | [RP] |
| RP-11 | Glottal T before a syllabic nasal (bitten [ˈbɪʔn̩]) | T → ∅ / V _ AH0 N # (low weight) | 1 | P | | [RP] |
| RP-12 | Conservative happY [ɪ] | IY0 → IH0 / _ # | 1 | P | Modern RP has a tense vowel, as GA does | [RP] |
| RP-13 | Lexical BrE/AmE differences | -ary/-ery/-ory lose the secondary-stressed vowel (military BrE /ˈmɪlɪtəri/; inventory, testimony); laboratory gets 2nd-syllable stress; *-ile* → AY L (fertile, mobile); *-ine* (carbine → AY N); *-berry* → B ER0 IY0; most two-syllable *-ate* verbs take final stress (rotate, translate); French loans take earlier stress | 2 | L | Needs word lists; ARPAbet stress digits cover the stress changes. Extra items not in [AmBr]: schedule (SH), tomato (AA), either (AY), privacy (IH), vitamin (IH), aluminium, herb (HH). These are **[unverified]**, though widely known. | [AmBr] |

### 1.5 US South

| ID | Feature | Rule | Sal | Type | Notes / lossiness | Src |
|---|---|---|---|---|---|---|
| SUS-01 | PRICE glide weakening | AY → AA / _ {voiced C, #}; in the Inland South and Texas, AY → AA everywhere | 3 | P | ride ≈ "rod/rad" [ɹaːd], but right [ɹaɪt]. [aː] falls between AA and AE; AE is an alternative output. | [SAE] [SVS] |
| SUS-02 | pin–pen merger | EH → IH / _ {M N NG} | 2 | P | Absent in New Orleans, southern Florida and the Lowcountry | [SVS] |
| SUS-03 | Drawl (breaking of short front vowels) | IH1 → IY1 AH0 ("pee-it"); EH1 → EY1 IH0 ("pay-it"); AE1 → AE1 Y AH0 ([æjə]) | 3 | P | Strongest on emphasised words; declining (mostly speakers born before 1960). Adds a syllable. | [SAE] [SVS] |
| SUS-04 | Southern Vowel Shift: FACE lowered [ɛɪ], FLEECE [ɪi], KIT [iə], DRESS [e(j)ə], TRAP [æ(j)ə] | Partly covered by SUS-03. FACE and FLEECE changes are NE. | 2 | P | Stage 1 across the whole South, stage 2 nearly all, stage 3 in the Inland South and central Texas | [SVS] |
| SUS-05 | Back-vowel fronting (GOOSE, GOAT, FOOT, STRUT, MOUTH onset) | NE | 2 | — | ARPAbet has no front rounded or central rounded vowels | [SAE] |
| SUS-06 | THOUGHT upglide [ɑɒ] | NE (a coda W is not valid). Strong setting: AO → OW (lossy, my inference) | 1 | — | | [SVS] |
| SUS-07 | fill–feel and fail–fell mergers | IY → IH / _ L; EY → EH / _ L | 1–2 | P | | [SVS] |
| SUS-08 | CHOICE → [ɔ] before /l/ | OY → AO / _ L (boil → "ball") | 1 | P | Most advanced speakers only | [SVS] |
| SUS-09 | -in' | IH0 NG → IH0 N | 2–3 | P | Strong in Appalachian English | [Gdrop] |
| SUS-10 | Initial stress | Move stress 1 to the first syllable in a word list (hotel, guitar) | 2 | L | [SAE] names hotel and guitar (features shared with AAVE). insurance, umbrella and police are **[unverified]**. Example: `HOTEL HH OW0 T EH1 L` → `HH OW1 T EH0 L`. | [SAE] |
| SUS-11 | Older non-rhotic Southern | R-deletion as RP-01, **without** linking R | 2 (old-fashioned) | P | Older White speakers in parts of Alabama, Savannah, Norfolk and New Orleans; modern Southern speech is strongly rhotic | [Rhot] [SAE] |
| SUS-12 | wh kept | W → HH W in wh-words | 1 | L | Inland South | [SVS] |

### 1.6 New Zealand

The front short vowels shift in a chain, TRAP → DRESS → KIT → central [ə] [NZE]. **Apply NZ-01..03 simultaneously.**

| ID | Feature | Rule | Sal | Type | Notes / lossiness | Src |
|---|---|---|---|---|---|---|
| NZ-01 | KIT centralised [ə] | IH1/IH2 → AH (not before NG) | 3 | P | Australians tease New Zealanders with "fush and chups". Before /ŋ/ KIT stays [ɪ]. | [NZE] |
| NZ-02 | DRESS raised [e̝~ɪ] | EH → IH | 3 | P | The source gives no "pen → pin" word examples | [NZE] |
| NZ-03 | TRAP raised [ɛ] | AE → EH | 2–3 | P | | [NZE] |
| NZ-04 | NEAR–SQUARE merger toward [iə] | EH R → IH R, then RP-01 (beer = bear) | 2 | P | Rose from 16% to 80% of teen speakers between 1983 and 1999 | [NZE] |
| NZ-05 | Non-rhotic, with linking and intrusive R | RP-01 and RP-02 | 3 | P | Southland is semi-rhotic: keep R in NURSE | [NZE] [Rhot] |
| NZ-06 | FACE [æe] | EY → AY (mild, lossy) | 2 | P | | [NZE] |
| NZ-07 | PRICE [ɑe], broad [ɒɪ] | AY → OY in broad speech | 2 | P | Rounded forms are stigmatised | [NZE] |
| NZ-08 | GOAT [ɐʉ] | OW → AW (lossy). With an unrounded offset some speakers' "no" sounds like "nigh" (OW → AY) | 2 | P | | [NZE] |
| NZ-09 | MOUTH [æo]; broad centring [ɛə] | NE. Broad: AW → EH AH0 (encroaches on SQUARE) | 1 | P | | [NZE] |
| NZ-10 | GOOSE central [ʉː] / [ɡəʉs]; FLEECE [fləis]; NURSE rounded [øː] | NE | 2 | — | | [NZE] |
| NZ-11 | Vowel mergers before L | EH → AE / _ L (Ellen = Alan); OW → AA / _ L (dole = doll) | 1–2 | P | | [NZE] |
| NZ-12 | L-vocalisation | As CKN-06 (ball [boːʊ̯]) | 2 | P | | [NZE] [Lvoc] |
| NZ-13 | BATH, including before nasals | RP-03 plus dance, chance, plant, grant | 2 | L | Some older Southland speakers keep TRAP | [NZE] [TrapBath] |
| NZ-14 | TH-fronting (younger speakers) | TH → F, mostly word-final | 1 | P | About half of final tokens in Wood 2003 | [NZE] |
| NZ-15 | FOOT centralised (good [ɡəd]) | UH → AH (low weight) | 1 | P | | [NZE] |
| NZ-16 | Full weak-vowel merger | IH0 → AH0 | 1 | P | | [AuE] |

### 1.7 Australian

Realisations run cultivated → general → broad; scale the probabilities by "broadness".

| ID | Feature | Rule | Sal | Type | Notes / lossiness | Src |
|---|---|---|---|---|---|---|
| AUS-01 | FACE [æ̠ɪ], broad [æ̠ːɪ~a̠ːɪ] | EY → AY (today ≈ "to die"; mate ≈ "mite") | 3 | P | | [AuE] |
| AUS-02 | PRICE [ɒe], broad [ɒːe] | AY → OY | 3 | P | The spelling "noice" is **[unverified]** | [AuE] |
| AUS-03 | GOAT [æ̠ʉ] (general), [æ̠ːʉ~a̠ːʉ] (broad) | OW → AW (lossy) | 2 | P | South Australia has an [ɔy] variant | [AuE] |
| AUS-04 | MOUTH [æo], broad [ɛːo] | NE | 2 | — | | [AuE] |
| AUS-05 | FLEECE [ɪ̈i], broad [əːɪ]; GOOSE [ɪ̈ɯ], broad [əːʉ]; NURSE [ɵː] | NE. Caricature: IY → EY at low weight | 1–2 | — | | [AuE] |
| AUS-06 | Non-rhotic, with linking and intrusive R (drawing → "draw-ring", saw it → "sore it") | RP-01 and RP-02 | 3 | P | | [AuE] [Rhot] |
| AUS-07 | BATH | RP-03 for path, laugh, class; before nasals (dance, chance) mostly AE, except South Australia | 2 | L | 86% of Adelaide speakers use the PALM vowel in *graph*; Hobart speakers use TRAP | [AuE] [TrapBath] |
| AUS-08 | Flapping of T, and of D for some speakers | none (same as GA). metal = medal for some | — | — | | [AuE] [Flap] |
| AUS-09 | L-vocalisation (milk [mɪʊ̯k]) | Coda L → UH0 | 1 | P | More common in South Australia | [AuE] |
| AUS-10 | Raised TRAP before nasals (ban [beːn] vs Ben [ben]) | AE → EH / _ {M N} (lossy: length is NE) | 1 | P | | [AuE] |
| AUS-11 | salary–celery merger | EH → AE / _ L | 1 | P | Some Victorian speakers | [AuE] |
| AUS-12 | Weak-vowel merger | IH0 → AH0, except before velars | 1 | P | | [AuE] |
| AUS-13 | KIT closer, near [i] | IH → IY (low weight) | 1 | P | | [AuE] |
| AUS-14 | Yod-coalescence (dune = June) | as IRL-26 | 1 | L | | [AuE] |

### 1.8 Inland North (Northern Cities Shift)

The ordering below follows [NCVS] ("often but not always" in this order). **Apply NCS-01..06 simultaneously.**

| ID | Feature | Rule | Sal | Type | Notes / lossiness | Src |
|---|---|---|---|---|---|---|
| NCS-01 | TRAP raised and tensed, all environments: [ɛə~eə], at most [ɪə] | Mild: AE → EH. Strong: AE1 → EY1 AH0 or IY1 AH0 (adds a syllable) | 3 | P | naturally [ˈneətʃɹəli] | [NCVS] |
| NCS-02 | LOT fronted to [ä~a] | AA → AE / _ (not R) | 3 | P | stock ≈ "stack", botch ≈ "batch" | [NCVS] |
| NCS-03 | THOUGHT lowered | AO → AA | 2 | P | caught ≈ unshifted "cot" | [NCVS] |
| NCS-04 | DRESS backed or lowered, toward [ɐ] or almost [æ] | EH → AH (backing) or EH → AE (lowering) | 2 | P | | [NCVS] |
| NCS-05 | STRUT backed toward [ɔ] | AH1 → AO1 | 2 | P | bus ≈ "boss" | [NCVS] |
| NCS-06 | KIT lowered/backed to [ɘ~ə], or [e] | IH1 → AH1 (or IH1 → EH1) | 1 | P | | [NCVS] |
| NCS-07 | "American raising" of AY before voiceless consonants | NE | 1 | — | Most consistent in the Inland North | [CanR] |
| NCS-08 | Working-class TH-stopping (Chicago, Detroit, Milwaukee, Cleveland, Buffalo) | DH → D / # _, TH → T | 2 | P | van den Doel 2006 via [ThStop] | [ThStop] |

### 1.9 Jamaican English (acrolect), with Patois-leaning options

| ID | Feature | Rule | Sal | Type | Notes / lossiness | Src |
|---|---|---|---|---|---|---|
| JAM-01 | TH-stopping | TH → T, DH → D (think /tiŋk/, this /dɪs/) | 3 | P | Varies by social class | [JamE] [CarE] |
| JAM-02 | H-dropping, plus [h] inserted at hiatus | HH → ∅ / # _; also low-probability +HH / # _ V1 | 2 | P | In central and eastern varieties, *hand* and *and* can both be [han] or [an]. In western varieties /h/ is a full phoneme. | [JamP] [CarE] [Hdrop] |
| JAM-03 | Final cluster reduction | S T → S, S K → S (desk → "dess"), N D → N, K T → K, L D → L, F T → F / _ # (best, respect, land) | 2 | P | | [CarE] [CCR] |
| JAM-04 | Semi-rhotic | R → ∅ / _ C; R → ∅ in unstressed final syllables (over: ER0 → AH0 or AA0). Keep prevocalic R and R in stressed final syllables (there) | 2 | P | Wells 1982 via [Rhot]. Educated speech is about 21.7% rhotic. | [Rhot] [JamE] |
| JAM-05 | Palatalised K/G before /a/ | +Y / {K G} _ {AA AE} (car [kjaːɹ], cat [kjat], gap [ɡjap]) | 3 | P | Middle-class speakers tend to avoid it | [JamE] |
| JAM-06 | TRAP [a]; BATH/PALM/START [aː] | AE → AA (lossy: [a] falls between the two) | 2 | P | | [JamE] |
| JAM-07 | LOT/THOUGHT rounded [ɔ] | AA → AO / _ (not R; LOT only) | 1–2 | P | | [JamE] |
| JAM-08 | STRUT [ɵ~o] | AH1 → UH1 (mild) or AO1 (strong) | 2 | P | lossy | [JamE] |
| JAM-09 | MOUTH [ɵʊ] | AW → OW (town ≈ "tone") | 2 | P | lossy | [JamE] |
| JAM-10 | FACE/GOAT: monophthongs [eː]/[oː] in the acrolect; ingliding [ie]/[uo] in the basilect | Acrolect: NE, keep EY/OW. Patois: OW1 → UW1 AH0 (token [ˈtuoken], boat /buat/); EY1 → IY1 EH0 | 3 (Patois) | P | Adds a syllable. GOAT [uo] is attested [JamE] [JamP]. **FACE [ie] is my inference** from the JC diphthong inventory (/ia/, /ua/ ...) [JamP] and fair/fear [iɛɹ] [JamE]; I did not find it stated directly. | [JamE] [JamP] [Harry2006] |
| JAM-11 | fair–fear merger [eːɹ~iɛɹ] | EH R → IH R (bear = beer) | 1 | P | | [JamE] |
| JAM-12 | PRIZE–CHOICE merger to [ɔi] | AY → OY (line = loin) | 2 | P | "Presumably less-educated" Patois speakers (Devonish & Harry 2004:465) | [JamE] |
| JAM-13 | TRAP–LOT merger (rat = rot) | AE → AO (as the source states, merged to [ɔ]) | 1–2 | P | **Check this.** [JamE] says the merged vowel is [ɔ] and cites Devonish & Harry 2004:465. I expected [a], so the direction is worth verifying. | [JamE] |
| JAM-14 | Unreduced schwa [a~ɐ] | AH0 → AA0 (about, bacon) | 2 | P | | [JamE] [CarE] |
| JAM-15 | Shifted stress (reaLISE, celeBRATE, agriCULture) | Move stress 1 to the final syllable of verbs in -ise/-ize/-ate and similar | 2 | L/P | | [CarE] |
| JAM-16 | Alveolar → velar before syllabic L | T → K, D → G / V _ AH0 L # (bottle /bakl̩/, idle /aiɡl̩/) | 2 | P | Basilect; Cassidy 1971 via [JamP] | [JamP] |
| JAM-17 | Implosive voiced stops; less stress contrast | NE | — | — | | [JamP] [JamE] |
| JAM-18 | -in' | IH0 NG → IH0 N | 2 | P | **No Jamaica-specific source found**; this rests on Wells's general working-class claim [Gdrop] | [Gdrop] |

### 1.10 L2 accents

These are transfer patterns. [NNP] attests some of them. Others are **predictions from the L1 inventory**, and I mark those as inference.

**German-accented English**

| ID | Feature | Rule | Sal | Type | Notes | Src |
|---|---|---|---|---|---|---|
| DE-01 | Final devoicing | B→P, D→T, G→K, V→F, DH→TH, Z→S, ZH→SH, JH→CH / _ # (also before a coda C) | 3 | P | cub = cup, had = hat | [NNP] [GerPh] |
| DE-02 | /w/ → [v] | W → V | 3 | P | | [NNP] |
| DE-03 | Hypercorrect /v/ → [w] | V → W (low probability) | 2 | P | | [NNP] |
| DE-04 | TH substitution | TH → S (or F); DH → Z (or D) | 3 | P | [NNP] gives /s/ or /f/ (think = sink, thought = fought). DH → Z ("ze") is my voiced extension. | [NNP] |
| DE-05 | TRAP = DRESS | AE → EH (pat = pet, mat = met) | 2 | P | Source is a commercial teaching blog | [PS-DE] |
| DE-06 | No native /dʒ ʒ/ | JH → CH, ZH → SH | 2 | P | Inference: German has /dʒ ʒ/ only in loanwords [GerPh] | [GerPh] |
| DE-07 | Coda R vocalised to [ɐ]; uvular onset [ʁ] | R → ∅ / V _ (C or #), ER0 → AH0. Uvular R is NE | 2 | P | Inference from the L1 [GerPh] | [GerPh] |
| DE-08 | Clear coda L; glottal stop before vowel-initial words; less vowel reduction; narrower pitch range | NE (reduction would need the spelling) | 1 | — | | [NNP] [GerPh] |
| DE-09 | Initial S + V → [z] (sing → "zing") | S → Z / # _ V | 1–2 | P | **[unverified]**: I couldn't confirm it in the part of [GerPh] I read | — |

**French-accented English**

| ID | Feature | Rule | Sal | Type | Notes | Src |
|---|---|---|---|---|---|---|
| FR-01 | H-deletion | HH → ∅, plus hypercorrect +HH before a vowel (hour, honest) | 3 | P | | [NNP] |
| FR-02 | TH substitution | France: TH → S, DH → Z. Quebec: TH → T, DH → D | 3 | P | | [NNP] |
| FR-03 | ship = sheep | IH → IY | 3 | P | | [NNP] |
| FR-04 | full = fool | UH → UW | 2 | P | Inference: French has only /i u/ as phonemes, not /ɪ ʊ/ [FrPh] | [FrPh] |
| FR-05 | Final stress | Make the last vowel of the word stress 1 and demote the others to 0 (or apply per phrase) | 3 | P | French stress falls on the final full syllable of the stress group [FrPh] | [FrPh] |
| FR-06 | No vowel reduction | AH0/IH0 → a full vowel based on the spelling | 2 | P | Needs the spelling. French vowels keep "their full quality in all syllables" [FrPh]. | [FrPh] |
| FR-07 | No native /tʃ dʒ/ | CH → SH, JH → ZH | 1–2 | P | Inference from [FrPh]; **unverified** as an L2 pattern | [FrPh] |
| FR-08 | Uvular /r/ [ʁ]; English /r/ perceived as /w/-like; monophthongal EY/OW | NE | 2 | — | | [NNP] [FrPh] |

**Spanish-accented English**

| ID | Feature | Rule | Sal | Type | Notes | Src |
|---|---|---|---|---|---|---|
| ES-01 | Prothetic e | +EH0 / # _ S C (speak → EH0 S P IY1 K; stomp [esˈtomp]) | 3 | P | | [NNP] [EsPh] |
| ES-02 | Five-vowel collapse | IH → IY; UH → UW; AE, AH, AA → AA; AO → OW | 3 | P | | [NNP] |
| ES-03 | b = v | V → B | 2 | P | | [NNP] |
| ES-04 | s = z | Z → S | 2 | P | Especially word-final (cheese, was, news) | [NNP] |
| ES-05 | ð/d | DH → D / not V _ V | 2 | P | Between vowels Spanish has [ð] anyway [EsPh] | [NNP] [EsPh] |
| ES-06 | θ | TH → T or S (Latin America) | 2 | P | | [NNP] |
| ES-07 | /dʒ ʒ/ = /j/ (Cuban, Central American) | JH → Y, ZH → Y | 2 | P | jump → "yump" | [NNP] |
| ES-08 | /ʃ/ ~ /tʃ/ merger | SH → CH, or CH → SH, depending on the dialect | 2 | P | [NNP] states the merger without a direction | [NNP] |
| ES-09 | Coda nasals neutralise; unfamiliar final consonants dropped or devoiced | M → N / _ #; delete or devoice finals other than S N R D L TH | 1 | P | | [NNP] |
| ES-10 | /h/ → [x] | NE: keep HH | 1 | — | Teaching sources only | — |

**Japanese-accented English**

| ID | Feature | Rule | Sal | Type | Notes | Src |
|---|---|---|---|---|---|---|
| JA-01 | l/r merger | L → R and R → L (both directions; emit both candidates) | 3 | P | Japanese has a single liquid [ɾ~l] | [NNP] [JaPh] [JaTr] |
| JA-02 | Vowel epenthesis | +UW0 after every coda consonant except N; +OW0 after T or D; +IY0 after CH or JH (match → *matchi*); older loans: +IY0 after a final K (strike → *sutoraiki*) | 3 | P | desk → *desuku*, bed → *beddo*, spring → *supuringu*. A 2019 Laboratory Phonology study found [ɯ] often used in every context. | [NNP] [Spaelti] [JaTr] [LabPhon2019] |
| JA-03 | TH substitution | TH → S (bathroom → *basurūmu*), but TH → SH before IY (thief → *shīfu*); DH → Z (the → *za*, mother → *mazā*) | 3 | P | | [JaTr] |
| JA-04 | b = v | V → B | 3 | P | | [JaPh] [JaTr] |
| JA-05 | Palatalisation before /i/ | S → SH, Z → JH / _ {IY IH} (cinema → *shinema*) | 3 | P | [si] and [zi] aren't established even in loanwords | [JaPh] |
| JA-06 | T and D before high vowels | T → CH / _ {IY IH} (older loans: team → *chīmu*); T → T S / _ UW (*tsu*); D → JH / _ IY (older loans) | 2 | P | Recent loans keep [ti di tu] | [JaPh] [JaTr] |
| JA-07 | HH before UW | HH → F / _ UW (who → *fū*). F itself is [ɸ], NE: keep F | 2 | P | | [JaPh] [JaTr] |
| JA-08 | Vowel mapping | AE → AA (map → *mappu*); AH1 → AA1 (muffin); AO → OW (talk → *tōku*); IH → IY; UH → UW; ER0/ER1 → AA (motor → *mōtā*); EY → EY or EH (name → *nēmu*) | 2–3 | P | Schwa follows the spelling (about → *abauto*), so it needs the orthography | [JaTr] [NNP] |
| JA-09 | Final NG | NG → N G UW0 / _ # (ring → *ringu*) | 2 | P | | [JaTr] |
| JA-10 | KAE → KYA | K AE → K Y AA (cat → *kyatto*) | 1 | P | | [JaTr] |
| JA-11 | Moraic timing, pitch accent, gemination | NE | 2 | — | | [NNP] [JaPh] |

---

## 2. Vowel-space coordinates (General American)

### 2.1 Data and conversion
- **Data**: Hillenbrand, Getty, Clark & Wheeler (1995), 12 vowels in /hVd/, spoken by 45 men and 48 women; formants are measured at the "steady state" and at 20%, 50% and 80% of vowel duration [H95]. I downloaded `vowdata.dat` from the authorised GitHub mirror [H95-data] and recomputed the averages. My raw means match Hillenbrand's own `vowdata.ds` (for example, men's /i/ F1 = 342.7 and F2 = 2322.8 Hz). The script is `/tmp/accents/scripts/h95_means.py`.
- **Conversion**:
  1. **Lobanov z-score per speaker** [Lobanov]. For each of the 93 adult talkers, z = (F − speaker's mean) / speaker's standard deviation, computed over that talker's 12 steady-state vowels, separately for F1 and F2.
  2. Average z across talkers.
  3. Min–max rescale over the 12 vowel means: **height = (zF1max − zF1)/(zF1max − zF1min)** and **backness = (zF2max − zF2)/(zF2max − zF2min)**. 0 = open / front, 1 = close / back. IY sets height 1 and backness 0, AA sets height 0, OW sets backness 1.
  4. The 20% and 80% points use the same per-speaker mean and SD, so they can fall slightly outside 0–1.
- **Important caveat**: Hillenbrand's talkers were mostly from southern Michigan (Inland North); his JIPA illustration of the dialect is titled "American English: Southern Michigan" [H03]. In these data **AE sits at the same height as EH (it is even marginally higher) and is fronter**, which is NCS-style TRAP raising, possibly helped by the /d/ context. STRUT is also rather back. For a "neutral GA" chart you may want to drop AE to height ≈ 0.2. **That adjustment is my suggestion, not a measurement.**
- **Rhoticity**: ER is defined by a very low F3 (men 1711 Hz vs 2355–3001 Hz for the other vowels; women 1930 vs 2735–3373 Hz), which a 2-D chart can't show. Add a third coordinate r (1 for ER and for V+R sequences, 0 elsewhere).

### 2.2 Coordinates

| ARPAbet | Lexical set | IPA (nominal) | F1/F2 men (Hz) | F1/F2 women (Hz) | Height | Backness | Round | Note |
|---|---|---|---|---|---|---|---|---|
| IY | FLEECE | i | 343/2323 | 437/2761 | 1.00 | 0.00 | 0 | |
| IH | KIT | ɪ | 429/2034 | 484/2369 | 0.84 | 0.22 | 0 | |
| EY | FACE | eɪ | 476/2090 | 535/2526 | 0.73 | 0.15 | 0 | steady state ≈ onset |
| EH | DRESS | ɛ | 588/1803 | 727/2063 | 0.40 | 0.39 | 0 | |
| AE | TRAP | æ | 591/1930 | 676/2335 | 0.44 | 0.26 | 0 | raised (see caveat) |
| AA | LOT/PALM | ɑ | 756/1309 | 921/1526 | 0.00 | 0.72 | 0 | |
| AO | THOUGHT | ɔ | 656/1023 | 804/1188 | 0.24 | 0.92 | 1 | merged with AA for many GA speakers |
| OW | GOAT | oʊ | 498/910 | 555/1036 | 0.68 | 1.00 | 1 | steady state |
| UH | FOOT | ʊ | 469/1123 | 519/1229 | 0.75 | 0.87 | 1 | |
| UW | GOOSE | u | 380/992 | 460/1106 | 0.93 | 0.95 | 1 | not fronted in these data |
| AH1/AH2 | STRUT | ʌ | 621/1181 | 760/1416 | 0.32 | 0.79 | 0 | measured "hud" |
| AH0 | commA (schwa) | ə | — | — | **0.59** | **0.58** | 0 | **Not measured** (Hillenbrand has stressed hVd only). Placed at the Lobanov origin (0,0), the speaker's vowel-space centroid, i.e. IPA mid-central [ə] put at the measured centre. |
| ER1/ER2 | NURSE | ɝ | 475/1379 | 524/1588 | 0.74 | 0.67 | (r = 1) | low F3; ER0 (lettER, [ɚ]) gets the same position |
| IH0 | roses | ɨ~ɪ | — | — | ~0.75 | ~0.45 | 0 | **Estimate** (IPA-based: higher and more central than IH). Not measured. |

**Diphthong targets** (start → end):

| ARPAbet | Start (h, b) | End (h, b) | Basis |
|---|---|---|---|
| EY | (0.73, 0.16) | (0.92, 0.06) | **Measured**: Hillenbrand 20% → 80% |
| OW | (0.61, 0.98) | (0.85, 1.02) | **Measured**: 20% → 80% |
| AY | (0.05, 0.50) [a] | (0.84, 0.22) ≈ IH | **IPA-based** placement in the measured frame. The /aɪ/ offset is "relatively consistently acoustically close to [ɪ]" [Shport2023]. |
| AW | (0.05, 0.50) [a] | (0.75, 0.87) ≈ UH | **IPA-based**. The /aʊ/ offset is highly variable across speakers [Shport2023]. |
| OY | (0.30, 0.92) [ɔ] | (0.84, 0.22) ≈ IH | **IPA-based** |

The measured data also show inglides on the lax vowels, i.e. 20% → 80% movement toward the centre: AE (0.44, 0.27) → (0.26, 0.46), UH (0.76, 0.87) → (0.69, 0.59), AH (0.32, 0.80) → (0.48, 0.56). Worth knowing if you model AE breaking (US South, NCS).

**IPA-chart alternative** (no measurement; the trapezoid simplified to a rectangle). Heights: close 1, near-close 0.83, close-mid 0.67, mid 0.5, open-mid 0.33, near-open 0.17, open 0. Backness: front 0, near-front 0.2, central 0.5, near-back 0.8, back 1.

| ARPAbet | IPA | Height | Backness |
|---|---|---|---|
| IY | i | 1 | 0 |
| IH | ɪ | 0.83 | 0.2 |
| EY | e → ɪ | 0.67 → 0.83 | 0 → 0.2 |
| EH | ɛ | 0.33 | 0 |
| AE | æ | 0.17 | 0 |
| AA | ɑ | 0 | 1 |
| AO | ɔ | 0.33 | 1 |
| OW | o → ʊ | 0.67 → 0.83 | 1 → 0.8 |
| UH | ʊ | 0.83 | 0.8 |
| UW | u | 1 | 1 |
| AH1 | ʌ | 0.33 | 1 (GA realisation is more central) |
| AH0 | ə | 0.5 | 0.5 |
| ER | ɝ | 0.5 | 0.5 (+r) |
| AY | a → ɪ | 0 → 0.83 | 0 → 0.2 |
| AW | a → ʊ | 0 → 0.83 | 0 → 0.8 |
| OY | ɔ → ɪ | 0.33 → 0.83 | 1 → 0.2 |

The measured chart is acoustic: rounding lowers F2, so rounded vowels look backer. That is arguably what a mishearing engine wants.

---

## 3. Shifts as geometry

**Chart orientation for every statement below**: front on the left, close at the top (the standard IPA layout). "Clockwise" refers to that picture. p = (b, h), and the centre is c = (0.58, 0.59).

**Primitive operations ("knobs")**
1. **Translate / pull**: p' = p + α(t − p) toward a target t. For example, "pull to the top-right" is t = (1, 1). Masks: tense-only, lax-only, front-only, stressed-only.
2. **Rigid rotation** about c by θ. Rarely right on its own (see below), because real shifts leave some vowels fixed.
3. **Perimeter rotation**: parametrise the convex hull of the monophthongs by arc length and move each *peripheral* vowel by Δs along it. This models chain shifts better than rigid rotation; interior vowels then move into vacated slots.
4. **Mirror front/back**: b' = 1 − b. IY ↔ UW, EH ↔ AO-ish, AE ↔ AA. Rounding doesn't follow automatically: treat it as a separate axis, then snap (step 9).
5. **Centralise / peripheralise**: p' = c + k(p − c), with k < 1 for reduction (NZ KIT, unstressed vowels) and k > 1 for hyperarticulation.
6. **Raise or lower** a class: h' = h ± δ.
7. **Front the back vowels**: b' = b − β for b > 0.5 and h > 0.5 (Labov's Principle III).
8. **Diphthong operations**: *monophthongise* (end := start; Southern PRICE); *lower the onset* (Principle IIa: Cockney, Australian and NZ FACE/PRICE/GOAT); *conditional onset raising* (Canadian raising); *add an inglide* (end := c; drawl, NCS TRAP).
9. **Snap**: after transforming, map each point to the nearest ARPAbet vowel (or diphthong path) by distance in (b, h, round, r). The near-ties are exactly where mishearings come from. Emitting n-best candidates weighted by distance (e.g. softmax over −d²) is a natural way to drive the word matcher.

**Labov's principles as built-in knobs.** Labov (1994:116) gives: "PRINCIPLE I: In chain shifts, long vowels rise. PRINCIPLE II: In chain shifts, short vowels fall. PRINCIPLE IIA: In chain shifts, the nuclei of upgliding diphthongs fall. PRINCIPLE III: In chain shifts, back vowels move to the front." His "pattern 4" (the Southern Shift) is "the raising of front short vowels and the lowering of the onsets of FLEECE and FACE", reported for the southern US, Australia, New Zealand and south-east England / London. Both are quoted in [Kerswill]. Labov says these are tendencies, not universals. They suggest four knobs: tense-up, lax-down, diphthong-onset-down and back-to-front. Turning them up together produces a generic "Southern Hemisphere / Cockney / Southern US" family resemblance.

| Shift | Cited movements | Geometric description | Where it fails |
|---|---|---|---|
| **Northern Cities** [NCVS] | TRAP rises along the front edge; LOT fronts along the bottom; THOUGHT lowers to the low back; DRESS backs or lowers; STRUT backs toward [ɔ]; KIT backs or lowers | **Clockwise perimeter rotation of the lower half, by about one slot**: left edge up (AE), bottom leftward (AA), right edge down (AO). Then a backward (rightward) translation of the interior lax vowels (EH, IH), with AH moving right into AO's old slot. My analysis; the source has no rotation language. | Not a rigid rotation: the top vowels (IY, UW, EY, OW) stay put, and AE travels farther than the others. AE gains an inglide (op 8). STRUT moves *against* the clockwise sense of an inner loop. |
| **Southern Vowel Shift** [SVS] | Stage 1: PRICE monophthongises, TRAP rises with glides. Stage 2: FACE onset lowers, DRESS rises with an inglide. Stage 3: KIT → [iə], FLEECE → [ɪi]. Back vowels and the MOUTH onset front; THOUGHT gets an upglide | Front half: **tense and lax vowels swap tracks**. The outer (tense) column moves down and inward; the inner (lax) column moves up and outward. That is a **counterclockwise loop in the front half** (Labov pattern 4). Plus op 8 (monophthongise AY), op 7 (front the back vowels) and inglides on the lax vowels. | The tense/lax crossing isn't representable as a rotation of points, because both members are still diphthongs. In ARPAbet, much of it is NE: [ɛɪ] and [ɪi] still snap to EY and IY. The audible ARPAbet consequences are the breaking insertions (SUS-03) and SUS-01. |
| **California** [CalE] | KIT → [ɛ], DRESS → [æ], TRAP → [ä] (lowered and backed; raised before nasals); LOT = THOUGHT; GOOSE → [ʉ] toward [y]; GOAT fronts; FOOT → [ʌ]; STRUT → [ɜ], sometimes [ɛ] | **Counterclockwise perimeter rotation on three sides**: the front edge moves down (IH→EH→AE), the bottom moves right (AE→[ä]), the top moves left (UW and OW front). Interior: FOOT goes down, STRUT forward. On the front edge and the bottom this runs **opposite to the NCS**. | The fourth side doesn't rotate: the low-back corner *collapses* (AO and AA merge) instead of moving up the back edge. Before nasals AE goes the other way (raising), so you need a conditional mask. Fronted GOOSE and GOAT are NE (snap back to UW and OW, or to IH UW). |
| **NZ front-vowel shift** [NZE] | TRAP → DRESS position [ɛ]; DRESS → [e̝~ɪ]; KIT → [ɪ̈] → [ə] | **Translate the front lax vowels up by one slot** (h + ~0.3). The top member, KIT, is pushed off the edge into the centre (op 5 with k ≈ 0). That is a clockwise loop on the front half (left edge up, top inward). | Fits well. In ARPAbet it gives the clean rules NZ-01..03. FLEECE gets an onglide (NE) to stay distinct from the raised DRESS. |
| **Great Vowel Shift** [GVS] | Long vowels raise one step: aː→ɛː→eː→(eɪ), ɛː→eː→iː, eː→iː, ɔː→oː→(oʊ), oː→uː. The high vowels diphthongise iː→əi→aɪ and uː→əu→aʊ, i.e. their onset falls through the centre | **Two mirror-image loops (a "fountain")**: up both edges, inward across the top, down the centre line. Clockwise on the front half, counterclockwise on the back half. Applied as a knob to modern GA: EY→IY, IY→AY, AO/OW→UW, UW→AW (modern vowels are already "post-shift", so this is a hypothetical second round). | Push chain vs drag chain is debated [GVS]. Only long vowels take part (in ARPAbet terms: IY EY AA AO OW UW, not IH EH AE UH AH). |
| **Reverse GVS** | Undo one step: IY→EY, EY→AA (or AE), AY→IY, UW→OW, OW→AO, AW→UW | Run the fountain backwards: down the edges, up the centre. | **Real-world partial reverses already exist**. Scots keeps ME ū (house → *hoose*: AW → UW) and OE ā (home → *hame*) [Scots]. Irish "tay" keeps ME ɛː as [eː] (IRL-03). The lexical-set problem: modern IY comes from both ME ē (meet) and ME ɛː (meat), and modern EY from both ME aː (mate) and ai (day), so an exact reverse needs etymology. The spelling helps a little (‹ea› = ɛː, ‹ee› = ē). |
| **Canadian raising** [CanR] | AY and AW onsets rise to [ʌ~ɐ~ə] before voiceless consonants in the same syllable (price, clout vs prize, cloud; writer [ˈɹʌɪɾɚ] vs rider [ˈɹaɪɾɚ]). Blocked in unstressed syllables before a stress (psychology) | **Conditional onset translation**: if a VOICELESS consonant follows in the same word and the vowel is stressed, then onset h += ~0.3 and b → 0.5. Raising of AY alone is "American raising" (Inland North, NYC, etc.). | It fits as geometry, but the result is NE in ARPAbet. The closest output for AW is OW ("about" ≈ "a boat", the same percept noted for SW Irish MOUTH [SWIrE]). For AY there is no substitute. The folk "aboot" is **[unverified]**. |

**Consonant knobs that recur across accents**

| Knob | ARPAbet rewrite | Accents (sources) |
|---|---|---|
| TH-stopping | TH → T, DH → D (dental [t̪ d̪] in some) | Irish (except Ulster); Caribbean/Jamaican; Nigerian, Liberian; Indian (dental, unmerged); working-class NYC, Inland North (Chicago, Detroit, Milwaukee, Cleveland, Buffalo), Upper Midwest and Mid-Atlantic (Philadelphia, Baltimore); AAVE (initial ð; *with*, *nothing*); Estuary and Sheffield (initial ð only); Cockney initial ð [Cockney]; L2 Quebec French, L2 Spanish ð [ThStop] [NNP] |
| TH → S/Z | TH → S, DH → Z | L2 German (also F), L2 French (France), L2 Japanese [NNP] [JaTr] |
| TH-fronting | TH → F; DH → V (not word-initially) | Cockney/London, Essex, Estuary, Bristol/West Country, Yorkshire, Manchester, Suffolk, Norwich, Glasgow, Edinburgh, AAVE, Liberian, Queensland (dying out), young NZ speakers [ThFront] [NZE] |
| Non-rhoticity (R-dropping) | R → ∅ / V _ (C or #), with vowel outputs as in RP-01; plus linking and intrusive R | RP, Cockney, Australian, NZ (except Southland), South African, older/coastal Southern US, AAVE, Boston/E. New England (receding), NYC (variable), most of the Caribbean (Jamaica semi-rhotic), Wales (mostly), traditional local Dublin, Louth, Cavan. Rhotic: Ireland, Scotland, West Country, parts of Lancashire, GA [Rhot]. L2 German (inference). |
| H-dropping | HH → ∅ / # _ | Most of England (Cockney, West Country, Midlands, North), Cardiff, some Jamaican/Caribbean, Newfoundland outside the Avalon peninsula; L2 French. **Absent** in Ireland, Scotland, North America, RP, East Anglia, Northumberland [Hdrop] [NNP]. Weak forms (he, him, have) everywhere. |
| Final devoicing | obstruent voiced → voiceless / _ # | L2 German [NNP]; L2 Spanish (finals) [NNP]; Scottish -ed → /t/ (SCO-11) [ScotE] |
| L-vocalisation | coda L → UH0, or ∅ after back rounded vowels | Cockney, Estuary, NYC, NZ, Pittsburgh, Philadelphia, Australian (esp. South Australia), Bristol, Plymouth, Bolton, AAVE (or deletion), some San Francisco speakers; before f/v for some GA speakers [Lvoc] [AuE]. Vowel mergers before L follow (NZ-11, SUS-07). |
| Flapping (matching knob: T ≈ D intervocalically) | none in 39 symbols (DX on the extended tier) | North America (near-obligatory), Australia, NZ, Ulster/Irish, Cardiff; rarer in Cockney, RP, South Africa [Flap]. Note: Scottish English, Scouse and old RP use [ɾ] for /r/ instead. |
| T-glottalling | T → Q (extended) → ∅ | Cockney, Estuary, modern RP (before syllabic n), colloquial Scottish (Glasgow), local Dublin, Ulster, some Australian, NZ (glottal reinforcement) [Cockney] [RP] [ScotE] [HibE] [AuE] [NZE] |
| Cluster reduction | delete the final stop in C C # (ST → S, ND → N, LD → L, KT → K) | AAVE (friend → "Ben", cold = coal), Caribbean/Jamaican, NZ and South Atlantic (Schreier), Chicano English (ward → "war"), Singapore, Nigerian, local Dublin [CCR] [CarE] [DubE]; L2 Spanish [NNP] |
| Epenthesis | +AH0 between sonorants; +EH0 before initial S C; +UW0/OW0/IY0 after codas | Irish (film, form), Scottish (girl, world, film), Nigerian (silk → "silik") [HibE] [ScotE] [CCR]; L2 Spanish prothesis [NNP]; L2 Japanese [JaTr] |
| G-dropping | IH0 NG → IH0 N / AH0 N | "Wherever there is an English-speaking working class" except South Africa (Wells); US South and Appalachia, AAVE, Cockney, casual RP and U-RP "huntin', shootin' and fishin'" [Gdrop]. Brummie, Mancunian and Scouse keep [ŋɡ] (no NG coalescence) [Gdrop]. |
| Yod changes | +Y after T D N (RP retention); T/D + UW → CH/JH (coalescence) | Conservative RP retention; coalescence in modern RP, Cockney, Irish, Australian [RP] [Cockney] [HibE] [AuE]. Lexical in CMUdict. |
| wh/w distinction | W → HH W (needs the spelling) | Scotland, Ireland (except advanced and supraregional Dublin), inland US South, older NZ, conservative RP [ScotE] [HibE] [SVS] [NZE] [RP] |
| Velar palatalisation | +Y / {K G} _ {AA AE} | Jamaican; traditional Ulster [JamE] [HibE]; L2 Japanese loans (*kyatto*) [JaTr] |

---

## 4. Sources

Wikipedia articles were read in November 2026. Each claim cites the specialist source named inside the article (I did not consult those directly).

- [HibE] Hiberno-English, Phonology section and tables (Hickey 2004/2007; Wells 1982; de Gruyter 2004): https://en.wikipedia.org/wiki/Hiberno-English
- [SWIrE] South-West Irish English (Hickey 2007; Wells 1982:425): https://en.wikipedia.org/wiki/South-West_Irish_English
- [DubE] Dublin English: https://en.wikipedia.org/wiki/Dublin_English
- [Brogue] Irish brogue (Ulster features): https://en.wikipedia.org/wiki/Irish_brogue
- [StageIr] Stage Irish: https://en.wikipedia.org/wiki/Stage_Irish
- [Hickey2010] Hickey, R. (2010). Irish English in early modern drama: the birth of a linguistic stereotype. In *Varieties of English in Writing*, 121–138 (abstract only): https://www.jbe-platform.com/content/books/9789027287786-veaw.g41.07hic
- [Maguire] Maguire, W. "The MEAT-MATE 'merger' in Mid-Ulster English revisited" (conference slides, not peer reviewed): https://api.sambuz.com/doc/the-meat-mate-merger-the-meat-mate-pdf-document-210870
- [Kelly2017] Kelly, N. E. (2017/2019). The perception of dental and alveolar stops among speakers of Irish English and American English. *English Language & Linguistics* 23(2): https://www.cambridge.org/core/product/A3ECBAEBB5EEE5223898284899D7DC66
- [HFV] Phonological history of English high front vowels (meet–meat, pin–pen, weak vowels; Wells 1982): https://en.wikipedia.org/wiki/Phonological_history_of_English_high_front_vowels
- [Wikt-tay] https://en.wiktionary.org/wiki/tay · [Wikt-ould] https://en.wiktionary.org/wiki/ould · [Wikt-Oirish] https://en.wiktionary.org/wiki/Oirish
- [ScotE] Scottish English: https://en.wikipedia.org/wiki/Scottish_English
- [Scots] Phonological history of Scots (Aitken 2002): https://en.wikipedia.org/wiki/Phonological_history_of_Scots
- [Cockney] Cockney (Wells 1982; Sivertsen 1960; Wright 1981): https://en.wikipedia.org/wiki/Cockney
- [RP] Received Pronunciation: https://en.wikipedia.org/wiki/Received_Pronunciation
- [AmBr] American and British English pronunciation differences: https://en.wikipedia.org/wiki/American_and_British_English_pronunciation_differences
- [TrapBath] Trap–bath split: https://en.wikipedia.org/wiki/Trap%E2%80%93bath_split
- [SAE] Southern American English (Labov, Ash & Boberg 2006; Thomas 2004): https://en.wikipedia.org/wiki/Southern_American_English
- [SVS] Southern vowel shift: https://en.wikipedia.org/wiki/Southern_vowel_shift
- [NZE] New Zealand English phonology (Bauer & Warren 2004; Gordon & Maclagan 2004; Hay, Maclagan & Gordon 2008): https://en.wikipedia.org/wiki/New_Zealand_English_phonology
- [AuE] Australian English phonology (Cox & Palethorpe; Harrington, Cox & Evans 1997; Cox & Fletcher 2017): https://en.wikipedia.org/wiki/Australian_English_phonology
- [NCVS] Northern cities vowel shift (Labov, Ash & Boberg 2006; Gordon 2004): https://en.wikipedia.org/wiki/Northern_cities_vowel_shift
- [JamE] Jamaican English (Devonish & Harry 2004; Wells 1982): https://en.wikipedia.org/wiki/Jamaican_English
- [JamP] Jamaican Patois, Phonology (Devonish & Harry 2004; Harry 2006; Cassidy 1971): https://en.wikipedia.org/wiki/Jamaican_Patois
- [Harry2006] Harry, O. G. (2006). Jamaican Creole. *JIPA* 36:125–131: https://www.cambridge.org/core/product/3BFA690953B7103696A225696A4A90B7
- [CarE] Caribbean English (Wells 1982:565–573; Allsopp 2003): https://en.wikipedia.org/wiki/Caribbean_English
- [NNP] Non-native pronunciations of English (German, French, Spanish, Japanese sections): https://en.wikipedia.org/wiki/Non-native_pronunciations_of_English
- [GerPh] Standard German phonology: https://en.wikipedia.org/wiki/Standard_German_phonology
- [FrPh] French phonology: https://en.wikipedia.org/wiki/French_phonology
- [EsPh] Spanish phonology: https://en.wikipedia.org/wiki/Spanish_phonology
- [JaPh] Japanese phonology: https://en.wikipedia.org/wiki/Japanese_phonology
- [JaTr] Transcription into Japanese (katakana renderings of English): https://en.wikipedia.org/wiki/Transcription_into_Japanese
- [Spaelti] Spaelti, P. (2005). Epenthetic *o* in Japanese loanwords (phonoloblog): https://quote.ucsd.edu/phonoloblog/?p=111
- [LabPhon2019] Vowel epenthesis in Japanese loanword adaptation, *Laboratory Phonology* (2019), abstract: https://doaj.org/article/cde0f37e046240d18a226e929c6be3da
- [PS-DE] Pronunciation Studio, "10 English Pronunciation Errors by German Speakers" (commercial teaching blog; weak source): https://pronunciationstudio.com/10-english-pronunciation-errors-german-speakers/
- [ThStop] Th-stopping: https://en.wikipedia.org/wiki/Th-stopping
- [ThFront] Th-fronting: https://en.wikipedia.org/wiki/Th-fronting
- [Hdrop] H-dropping: https://en.wikipedia.org/wiki/H-dropping
- [Lvoc] L-vocalization: https://en.wikipedia.org/wiki/L-vocalization
- [Flap] Flapping: https://en.wikipedia.org/wiki/Flapping
- [Rhot] Rhoticity in English: https://en.wikipedia.org/wiki/Rhoticity_in_English
- [Gdrop] G-dropping (Pronunciation of English ⟨ng⟩; Wells 1982; Hazen 2008): https://en.wikipedia.org/wiki/G-dropping
- [CCR] Cluster reduction: https://en.wikipedia.org/wiki/Cluster_reduction
- [CalE] California English (California Vowel Shift; Eckert): https://en.wikipedia.org/wiki/California_English
- [CanR] Canadian raising (Chambers; Labov, Ash & Boberg 2006): https://en.wikipedia.org/wiki/Canadian_raising
- [GVS] Great Vowel Shift (Lass 2000; Stockwell 2002): https://en.wikipedia.org/wiki/Great_Vowel_Shift
- [Chain] Chain shift (Cockney vowel shift iː → eɪ → aɪ → ɔɪ): https://en.wikipedia.org/wiki/Chain_shift
- [Kerswill] Kerswill, P., Torgersen, E. N. & Fox, S. "Reversing drift: Innovation and diffusion in the London diphthong system" (quotes Labov 1994:116 principles and the 1994:201–208 patterns): https://www.lancaster.ac.uk/fss/projects/linguistics/innovators/documents/Reversing_drift_Print_version_000.pdf
- [ARPA] ARPABET (extended symbols DX, Q, WH, AX, AXR, IX, UX, EL, EM, EN, NX): https://en.wikipedia.org/wiki/ARPABET
- [H95] Hillenbrand, J., Getty, L. A., Clark, M. J. & Wheeler, K. (1995). Acoustic characteristics of American English vowels. *JASA* 97(5):3099–3111: https://doi.org/10.1121/1.411872
- [H95-data] Data mirror, hosted with Hillenbrand's permission (vowdata.dat, vowdata.ds): https://github.com/santiagobarreda/hillenbrand_et_al_1995 (Hillenbrand's own page, https://homepages.wmich.edu/~hillenbr/voweldata.html, was unreachable from here)
- [H03] Hillenbrand, J. (2003). American English: Southern Michigan. *JIPA* illustration (I saw the title in search results only; the PDF host didn't resolve).
- [Lobanov] Lobanov, B. M. (1971). Classification of Russian vowels spoken by different speakers. *JASA* 49:606–608: https://doi.org/10.1121/1.1912396 ; formula documentation: https://search.r-project.org/CRAN/refmans/vowels/html/norm.lobanov.html
- [Shport2023] Shport, I., Bissell, M., Berkson, K. & Carmichael, K. (2023). Regional and individual variation in acoustic targets of /ai/ and /au/ in American English: https://vtechworks.lib.vt.edu/items/6a399220-7fd1-408a-b392-c46e9ad68c31

**Sites skipped or unreachable**: asian-efl-journal.com (Ohata 2004, Japanese L2; DNS failure), homepages.wmich.edu (DNS failure), core-prod.cambridgecore.org (DNS failure), the ICPhS 2023 PDF (404). No blocks were circumvented.
