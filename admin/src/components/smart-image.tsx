"use client";
import { useState, useRef, useEffect } from "react";

export default function SmartImage({
    src,
    width,
    height,
}: {
    src: string;
    width: number;
    height: number;
}) {
    const containerRef = useRef<HTMLDivElement>(null);
    const [containerWidth, setContainerWidth] = useState<number>(0);
    const [isLoading, setIsLoading] = useState(true);

    useEffect(() => {
        if (containerRef.current) {
            setContainerWidth(containerRef.current.offsetWidth);
        }
    }, []);

    let img = <></>;
    if (containerWidth > 0 && width) {
        let newWidth = width;
        let newHeight = height;
        if (width > containerWidth) {
            newHeight = Math.ceil(height * (containerWidth / width));
            newWidth = containerWidth;
        }
        img = (
            <img
                src={`${src}&width=${newWidth}&height=${newHeight}`}
                width={newWidth}
                height={newHeight}
                onLoad={() => setIsLoading(false)}
                onError={() => setIsLoading(false)}
            />
        );
    }

    return (
        <div ref={containerRef} className="w-full relative">
            {isLoading && (
                <div className="absolute inset-0 flex items-center justify-center bg-gray-100">
                    <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-gray-900"></div>
                </div>
            )}
            {img}
        </div>
    );
}
