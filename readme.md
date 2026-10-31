# Todo

- Look into `rkyv` for loading dictionary faster into web app
- We should be able to parse the Duplex form.
  - To do this we need an idea of "line similarity", which can be similar to rhyme distance, except that I think insertions/deletions incur no/low penalty, so that we can better line up the parts of the two lines that do correspond?
  - We want to do Smith-Waterman instead of the Needleman-Wunch that we do in @rhyme_distance.rs.
  - S-W finds the highest scoring local alignment.