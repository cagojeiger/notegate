module.exports = {
  ci: {
    collect: {
      staticDistDir: "./dist",
      isSinglePageApplication: true,
      url: ["http://localhost/"],
      numberOfRuns: 3,
      settings: {
        chromeFlags: process.env.CI ? "--no-sandbox" : "",
        onlyCategories: ["performance", "accessibility", "best-practices"]
      }
    },
    assert: {
      assertions: {
        "categories:performance": ["error", { minScore: 0.9, aggregationMethod: "optimistic" }],
        "categories:accessibility": ["error", { minScore: 1, aggregationMethod: "optimistic" }],
        "categories:best-practices": ["error", { minScore: 0.9, aggregationMethod: "optimistic" }],
        "largest-contentful-paint": ["error", { maxNumericValue: 2500, aggregationMethod: "optimistic" }],
        "cumulative-layout-shift": ["error", { maxNumericValue: 0.1, aggregationMethod: "optimistic" }],
        "total-blocking-time": ["error", { maxNumericValue: 200, aggregationMethod: "optimistic" }]
      }
    },
    upload: {
      target: "filesystem",
      outputDir: "./lighthouse-reports"
    }
  }
};
