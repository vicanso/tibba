import { getSelectedApp, setSelectedApp } from "@/storage";
import { create } from "zustand";
export interface PageNavigation {
    url?: string;
    icon?: React.JSX.Element;
    title: string;
}

const pageHeaderNavigation: PageNavigation[] = [];

export interface App {
    name: string;
    label: string;
}

interface BasicState {
    apps: App[];
    selectedApp: string;
    pageHeaderNavigation: PageNavigation[];
    setPageHeaderNavigation: (nav: PageNavigation[]) => void;
    resetPageHeaderNavigation: () => void;
    setTitle: (title: string) => void;
    selectApp: (app: string) => void;
}

const defaultApps: App[] = [
    {
        name: "tibbaWeb",
        label: "Tibba Web",
    },
    {
        name: "tibbaAdmin",
        label: "Tibba Admin",
    },
];

function getDefaultApp() {
    const app = getSelectedApp();
    if (app) {
        return app;
    }
    return defaultApps[0].name;
}

const useBasicState = create<BasicState>((set) => ({
    apps: defaultApps,
    pageHeaderNavigation,
    selectedApp: getDefaultApp(),
    setTitle: (title: string) => {
        set({
            pageHeaderNavigation: [
                {
                    title,
                },
            ],
        });
    },
    selectApp: (app: string) => {
        set({
            selectedApp: app,
        });
        setSelectedApp(app);
    },
    setPageHeaderNavigation: (nav: PageNavigation[]) => {
        set({
            pageHeaderNavigation: nav,
        });
    },
    resetPageHeaderNavigation: () => {
        set({
            pageHeaderNavigation: [],
        });
    },
}));

export default useBasicState;
