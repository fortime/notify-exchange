const pathRewrite = {};
pathRewrite['^' + process.env.VUE_APP_API_BASE_PATH] = '';

const devServerProxy = {};
devServerProxy['^' + process.env.VUE_APP_API_BASE_PATH] = {
  target: process.env.VUE_APP_DEV_SERVER_TARGET,
  pathRewrite: pathRewrite,
  changeOrigin: true,
  logLevel: 'debug'
};

module.exports = {
  publicPath: process.env.VUE_APP_PUBLIC_PATH,
  devServer: {
    allowedHosts: [
      process.env.VUE_APP_DEV_SERVER_HOST
    ],
    proxy: devServerProxy,
    client: {
      webSocketURL: process.env.VUE_APP_WS_URL,
    },
    webSocketServer: {
      type: 'ws',
      options: {
        path: process.env.VUE_APP_WS_PATH,
      },
    }
  }
};
