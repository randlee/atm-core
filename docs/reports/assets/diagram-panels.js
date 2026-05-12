(function () {
  function main() {
    var buttons = Array.prototype.slice.call(
      document.querySelectorAll("[data-copy-text]"),
    );
    buttons.forEach(function (button) {
      button.addEventListener("click", function () {
        var copyText = button.getAttribute("data-copy-text") || "";
        navigator.clipboard.writeText(copyText).then(
          function () {
            button.setAttribute("title", "Copied");
          },
          function () {
            button.setAttribute("title", "Copy failed");
          },
        );
      });
    });

    var schemaButtons = Array.prototype.slice.call(
      document.querySelectorAll("[data-dialog-id]"),
    );
    schemaButtons.forEach(function (button) {
      button.addEventListener("click", function () {
        var dialogId = button.getAttribute("data-dialog-id");
        if (!dialogId) {
          return;
        }
        var dialog = document.getElementById(dialogId);
        if (!dialog || typeof dialog.showModal !== "function") {
          return;
        }
        dialog.showModal();
      });
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", main);
  } else {
    main();
  }
})();
