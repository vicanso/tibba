export interface ModelViewOptions {
    limit: number;
    hiddenColumns: string[];
    order_by: string;
}

export function getModelViewOptions(name: string): ModelViewOptions {
    const options = localStorage.getItem(`${name}-view-options`);
    return options
        ? JSON.parse(options)
        : { limit: 10, hiddenColumns: [], order_by: "" };
}

export function setModelViewOptions(name: string, options: ModelViewOptions) {
    localStorage.setItem(`${name}-view-options`, JSON.stringify(options));
}

export function getSelectedApp() {
    return localStorage.getItem("selected-app");
}

export function setSelectedApp(app: string) {
    localStorage.setItem("selected-app", app);
}
