export default {
  extends: ['@commitlint/config-conventional'],
  rules: {
    // Descriptions mix Chinese and English and often start with proper nouns (Google, OpenAI, API),
    // so the default "no sentence-case subject" rule gives false positives.
    'subject-case': [0],
  },
};
