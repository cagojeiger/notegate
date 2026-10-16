window.onload = function () {
  window.ui = SwaggerUIBundle({
    url: "/openapi/v2.json",
    dom_id: "#swagger-ui",
    deepLinking: true,
    queryConfigEnabled: false,
    validatorUrl: null,
    persistAuthorization: false,
    presets: [SwaggerUIBundle.presets.apis, SwaggerUIStandalonePreset],
    plugins: [SwaggerUIBundle.plugins.DownloadUrl],
    layout: "StandaloneLayout",
  });
};
