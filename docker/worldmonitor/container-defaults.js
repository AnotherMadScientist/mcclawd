// container-defaults.js — Pre-app configuration for WorldMonitor container
// Loaded BEFORE the app initializes. Sets localStorage defaults on first visit.
// Lives OUTSIDE app/ so git pull inside app/ never conflicts.

(function () {
  'use strict';

  // ── Apple Silicon: force high-performance GPU for WebGL ──────────────
  const origGetContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function (type, attrs) {
    if (type === 'webgl' || type === 'webgl2') {
      attrs = Object.assign({}, attrs, { powerPreference: 'high-performance' });
    }
    return origGetContext.call(this, type, attrs);
  };

  // ── First-visit defaults ────────────────────────────────────────────
  if (!localStorage.getItem('wm-container-initialized')) {
    // 3D globe mode
    localStorage.setItem('worldmonitor-map-mode', 'globe');

    // All info panels below the map (correct app keys)
    var bottomPanels = JSON.stringify([
      'live-news',
      'live-webcams',
      'insights',
      'strategic-posture',
      'cii',
      'strategic-risk',
      'sirens',
      'telegram',
      'markets',
      'clocks',
      'youtube',
    ]);
    localStorage.setItem('panel-order-bottom', bottomPanels);
    localStorage.setItem('panel-order-bottom-set', 'true');

    // Clear right-side panel order so nothing stays beside the map
    localStorage.setItem('panel-order', '[]');

    localStorage.setItem('wm-container-initialized', '1');
  }

  // ── Disable globe auto-rotation ─────────────────────────────────────
  window.addEventListener('load', function () {
    var attempts = 0;
    var maxAttempts = 15;
    var interval = setInterval(function () {
      attempts++;

      // Globe.gl stores the Three.js controls on the globe instance
      // Look for the globe wrapper and its internal controls
      var globeViz = document.querySelector('.globe-viz');
      if (globeViz) {
        // Access the globe instance via __globeObjRef or scene children
        var canvases = document.querySelectorAll('canvas');
        canvases.forEach(function (canvas) {
          // Three.js OrbitControls are typically on the renderer's domElement parent
          var controls =
            canvas.__orbitControls ||
            canvas.parentElement?.__orbitControls ||
            canvas.__controls;
          if (controls && typeof controls.autoRotate !== 'undefined') {
            controls.autoRotate = false;
            controls.autoRotateSpeed = 0;
          }
        });
      }

      // Also try to intercept via Globe.gl's globeInstance
      if (window.__globeInstance) {
        var ctrl = window.__globeInstance.controls();
        if (ctrl) {
          ctrl.autoRotate = false;
          ctrl.autoRotateSpeed = 0;
        }
      }

      // Monkey-patch OrbitControls.autoRotate setter to prevent re-enabling
      if (attempts === 1) {
        try {
          var canvases = document.querySelectorAll('canvas');
          canvases.forEach(function (canvas) {
            if (canvas.__r3f) {
              // React Three Fiber scene — intercept at invalidate level
              var scene = canvas.__r3f;
              if (scene.controls) {
                Object.defineProperty(scene.controls, 'autoRotate', {
                  get: function () {
                    return false;
                  },
                  set: function () {
                    /* noop */
                  },
                  configurable: true,
                });
              }
            }
          });
        } catch (e) {
          /* non-critical */
        }
      }

      if (attempts >= maxAttempts) {
        clearInterval(interval);
      }
    }, 2000);
  });

  // ── Curated X/Twitter accounts for the LHS overlay ──────────────────
  // Other scripts can read this via window.__wmConfig
  window.__wmConfig = {
    xAccounts: [
      'ABORINTL',
      'Conflicts',
      'IntelCrab',
      'AuroraIntel',
      'sentdefender',
      'War_Mapper',
      'Liveuamap',
      'TheStudyofWar',
      'RALee85',
      'JulianRoepcke',
    ],
    regionHashtags: {
      MENA: ['#Iran', '#Israel', '#MiddleEast', '#Syria', '#Gaza'],
      Europe: ['#Ukraine', '#NATO', '#Russia', '#EU'],
      Asia: ['#Taiwan', '#NorthKorea', '#SouthChinaSea', '#IndoPacific'],
      Africa: ['#Sahel', '#WestAfrica', '#Sudan', '#Ethiopia'],
      Americas: ['#LatinAmerica', '#Venezuela', '#Mexico', '#Caribbean'],
      Global: ['#OSINT', '#Breaking', '#Geopolitics', '#Security'],
    },
    defaultRegion: 'Global',
  };
})();
